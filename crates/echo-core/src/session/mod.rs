//! Die Verbindung zum eigenen Server, wie Oberflaeche und Audio sie
//! brauchen, ohne QUIC zu kennen. Zustand kommt als unveraenderlicher
//! [`ServerSnapshot`] plus Ereignisqueue; Befehle sind async und liefern
//! Fehler als Wert, zusaetzlich als [`EventKind::Error`] fuer den Toast.
//!
//! Die Oberflaeche liest Schnappschuss und Ereignisse je Frame und
//! blockiert nie. Netzwerk laeuft auf tokio; `send_voice` geht synchron vom
//! Aufnahme-Thread.

mod commands;

pub use commands::FileListing;
mod handler;

use std::collections::{HashMap, VecDeque};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use bytes::Bytes;
use echo_protocol::{
    ChannelDto, ClientDto, Envelope, FrameKind, Identity, Message, PasswordProof, StreamDto,
    VoiceFrame, Wire, encode, hello_payload, read_message, write_message,
};
use echo_storage::{ClientDatabase, HostVerdict, file_key};
use parking_lot::Mutex;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::address::parse_address;
use crate::events::{ConnectionState, EventKind, SessionEvent};
use crate::mirror::ChatMirror;
use crate::round_trip::RoundTripMeter;
use crate::snapshot::ServerSnapshot;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const PING_INTERVAL: Duration = Duration::from_secs(10);
const IDLE_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_VOICE_IN_FLIGHT: u32 = 64;
const MAX_VOICE_PACKET: usize = Wire::MAX_VOICE_FRAME_BYTES + VoiceFrame::HEADER_SIZE;

/// Womit verbunden wird; ein Favorit ohne Verlauf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectOptions {
    pub address: String,
    pub nickname: String,
    pub server_password: Option<String>,
    pub default_channel: Option<u64>,
    pub default_channel_password: Option<String>,
}

impl ConnectOptions {
    #[must_use]
    pub fn new(address: &str, nickname: &str) -> Self {
        Self {
            address: address.to_owned(),
            nickname: nickname.to_owned(),
            server_password: None,
            default_channel: None,
            default_channel_password: None,
        }
    }
}

/// Was die Session von aussen braucht.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub identity: Arc<Identity>,
    /// Fuer `known_hosts` (echo-2).
    pub client_db: Arc<ClientDatabase>,
    /// Wohin die Spiegel der Verlaeufe kommen (je Server ein Ordner); ohne
    /// Spiegel laeuft der Chat nur mit dem, was der Server schickt.
    pub mirror_root: Option<PathBuf>,
}

/// Ein empfangener Sprachframe. Kommt vom Netzwerk, nicht unbedingt in
/// Reihenfolge; geordnet wird im Jitter-Puffer (echo-141). Leere Nutzlast
/// heisst: Ende des Sprechens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedVoice {
    pub kind: FrameKind,
    pub sender: u16,
    pub sequence: u16,
    pub timestamp_ms: u32,
    pub opus: Vec<u8>,
}

type Callback = Arc<dyn Fn() + Send + Sync>;
type VoiceSink = Arc<dyn Fn(ReceivedVoice) + Send + Sync>;

/// Der Stand, aus dem Schnappschuesse entstehen.
#[derive(Debug, Default)]
struct Model {
    server_name: String,
    welcome_message: String,
    own_id: u16,
    channels: HashMap<u64, ChannelDto>,
    clients: HashMap<u16, ClientDto>,
    streams: HashMap<u16, StreamDto>,
}

/// Eine laufende Verbindung.
struct Live {
    connection: quinn::Connection,
    endpoint: quinn::Endpoint,
    runtime: tokio::runtime::Handle,
    outbox: mpsc::UnboundedSender<Vec<u8>>,
    pending: Mutex<HashMap<u32, oneshot::Sender<Message>>>,
    next_request: AtomicU32,
    cancel: CancellationToken,
    /// Warum der Server uns geworfen hat, fuer die Meldung beim Trennen.
    kicked: Mutex<Option<String>>,
    started: Instant,
    voice_in_flight: Arc<AtomicU32>,
}

struct Shared {
    config: SessionConfig,
    snapshot: ArcSwap<ServerSnapshot>,
    model: Mutex<Model>,
    events: Mutex<VecDeque<SessionEvent>>,
    state: AtomicU8,
    round_trip: RoundTripMeter,
    on_change: Mutex<Option<Callback>>,
    on_voice: Mutex<Option<VoiceSink>>,
    live: Mutex<Option<Arc<Live>>>,
    voice_sequence: AtomicU16,
    version: AtomicU64,
    mirror: Mutex<Option<Arc<ChatMirror>>>,
    peer_names: Mutex<HashMap<String, String>>,
    host_fingerprint: Mutex<String>,
    address: Mutex<String>,
}

/// Die Session; billig zu klonen, alle Klone teilen eine Verbindung.
#[derive(Clone)]
pub struct Session {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

impl Session {
    #[must_use]
    pub fn new(config: SessionConfig) -> Self {
        Self {
            shared: Arc::new(Shared {
                config,
                snapshot: ArcSwap::from_pointee(ServerSnapshot::default()),
                model: Mutex::new(Model::default()),
                events: Mutex::new(VecDeque::new()),
                state: AtomicU8::new(0),
                round_trip: RoundTripMeter::default(),
                on_change: Mutex::new(None),
                on_voice: Mutex::new(None),
                live: Mutex::new(None),
                voice_sequence: AtomicU16::new(0),
                version: AtomicU64::new(0),
                mirror: Mutex::new(None),
                peer_names: Mutex::new(HashMap::new()),
                host_fingerprint: Mutex::new(String::new()),
                address: Mutex::new(String::new()),
            }),
        }
    }

    // --- Lesen ----------------------------------------------------------------

    #[must_use]
    pub fn state(&self) -> ConnectionState {
        match self.shared.state.load(Ordering::Acquire) {
            1 => ConnectionState::Connecting,
            2 => ConnectionState::Connected,
            _ => ConnectionState::Disconnected,
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> Arc<ServerSnapshot> {
        self.shared.snapshot.load_full()
    }

    /// Geglaettete Antwortzeit zum Server (echo-92).
    #[must_use]
    pub fn round_trip(&self) -> Option<Duration> {
        self.shared.round_trip.current()
    }

    /// Der Fingerabdruck des Servers der laufenden Verbindung.
    #[must_use]
    pub fn host_fingerprint(&self) -> String {
        self.shared.host_fingerprint.lock().clone()
    }

    #[must_use]
    pub fn address(&self) -> String {
        self.shared.address.lock().clone()
    }

    /// Das naechste Ereignis; die UI leert die Queue je Frame.
    #[must_use]
    pub fn next_event(&self) -> Option<SessionEvent> {
        self.shared.events.lock().pop_front()
    }

    /// Wird aus einem Netzwerk-Thread gerufen, wenn es Neues gibt; die UI
    /// weckt damit ihre Frame-Schleife.
    pub fn on_change(&self, callback: impl Fn() + Send + Sync + 'static) {
        *self.shared.on_change.lock() = Some(Arc::new(callback));
    }

    /// Wohin empfangene Sprachframes gehen (Audio-Engine).
    pub fn on_voice(&self, sink: impl Fn(ReceivedVoice) + Send + Sync + 'static) {
        *self.shared.on_voice.lock() = Some(Arc::new(sink));
    }

    // --- Verbinden ------------------------------------------------------------

    /// Verbindet; `Err` mit dem Grund. Der Grund kommt zugleich als
    /// `Disconnected`-Ereignis, ein geaenderter Host-Schluessel als
    /// `HostKeyChanged`.
    pub async fn connect(&self, options: ConnectOptions) -> Result<(), String> {
        self.disconnect().await;
        self.shared.address.lock().clone_from(&options.address);
        self.set_state(ConnectionState::Connecting);
        let (host, port) = parse_address(&options.address);
        self.publish(EventKind::Info(format!("Verbinde mit {host}:{port} …")));

        match self.establish(&options, &host, port).await {
            Ok(()) => Ok(()),
            Err(failure) => {
                self.set_state(ConnectionState::Disconnected);
                self.publish(EventKind::Disconnected {
                    reason: failure.reason.clone(),
                    was_error: true,
                });
                if let Some(changed) = failure.host_key_changed {
                    self.publish(*changed);
                }
                Err(failure.reason)
            }
        }
    }

    /// Nimmt nach `HostKeyChanged` genau den gezeigten Schluessel an; zeigt
    /// der Server beim naechsten Versuch wieder einen anderen, wird erneut
    /// gefragt.
    pub fn trust_host_key(&self, host: &str, offered: &str) -> Result<(), String> {
        self.shared
            .config
            .client_db
            .remember_host(host, offered)
            .map_err(|e| e.to_string())
    }

    pub async fn disconnect(&self) {
        let live = self.shared.live.lock().take();
        let Some(live) = live else {
            return;
        };
        live.cancel.cancel();
        live.connection.close(0u32.into(), b"");
        live.endpoint.wait_idle().await;
        let was_connected = self.state() == ConnectionState::Connected;
        self.set_state(ConnectionState::Disconnected);
        if was_connected {
            self.publish(EventKind::Disconnected {
                reason: "Verbindung getrennt.".into(),
                was_error: false,
            });
        }
    }

    async fn establish(
        &self,
        options: &ConnectOptions,
        host: &str,
        port: u16,
    ) -> Result<(), Failure> {
        let target = resolve(host, port).await?;
        let endpoint = client_endpoint(target)?;
        let connecting = endpoint
            .connect(target, host)
            .map_err(|e| Failure::new(format!("QUIC: {e}")))?;
        let connection = tokio::time::timeout(REQUEST_TIMEOUT, connecting)
            .await
            .map_err(|_| Failure::new("Der Server antwortet nicht.".into()))?
            .map_err(|e| Failure::new(format!("QUIC: {e}")))?;

        let fingerprint = echo_quic::server_fingerprint(&connection)
            .ok_or_else(|| Failure::new("Server ohne Zertifikat.".into()))?;
        // Wie SSH: der Eintrag gilt je Host UND Port.
        let host_key = format!("{host}:{port}");
        self.check_host_key(&host_key, &fingerprint, options)
            .inspect_err(|_| {
                connection.close(0u32.into(), b"");
            })?;
        self.shared.host_fingerprint.lock().clone_from(&fingerprint);

        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|e| Failure::new(format!("QUIC: {e}")))?;
        let welcome = self
            .handshake(&mut send, &mut recv, options, &fingerprint)
            .await?;
        self.start(connection, endpoint, send, recv, welcome);
        Ok(())
    }

    fn check_host_key(
        &self,
        host_key: &str,
        fingerprint: &str,
        options: &ConnectOptions,
    ) -> Result<(), Failure> {
        let db = &self.shared.config.client_db;
        match db
            .check_host(host_key, fingerprint)
            .map_err(|e| Failure::new(e.to_string()))?
        {
            HostVerdict::Known => Ok(()),
            HostVerdict::New => {
                db.remember_host(host_key, fingerprint)
                    .map_err(|e| Failure::new(e.to_string()))?;
                self.publish(EventKind::Info(format!(
                    "Neuer Server, Host-Schlüssel gemerkt: {fingerprint}"
                )));
                Ok(())
            }
            HostVerdict::Changed => {
                let known = db
                    .host_fingerprint(host_key)
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                Err(Failure {
                    reason: format!(
                        "Der Host-Schlüssel von {host_key} hat sich geändert ({fingerprint})."
                    ),
                    host_key_changed: Some(Box::new(EventKind::HostKeyChanged {
                        host: host_key.to_owned(),
                        known,
                        offered: fingerprint.to_owned(),
                        options: Box::new(options.clone()),
                    })),
                })
            }
        }
    }

    /// `Open`, `Challenge`, `Hello`, `Welcome`.
    async fn handshake(
        &self,
        send: &mut quinn::SendStream,
        recv: &mut quinn::RecvStream,
        options: &ConnectOptions,
        host_fingerprint: &str,
    ) -> Result<Message, Failure> {
        write_message(
            send,
            &Envelope {
                request_id: 0,
                message: Message::Open {
                    version: Wire::VERSION,
                },
            },
        )
        .await
        .map_err(protocol)?;
        let Message::Challenge {
            nonce,
            version,
            app_version,
            needs_password,
            ..
        } = answer(recv).await?
        else {
            return Err(Failure::new("Challenge erwartet.".into()));
        };
        if version != Wire::VERSION {
            return Err(Failure::new(format!(
                "Der Server spricht Protokoll {version} (echo-server {app_version}), dieser Client Protokoll {}.",
                Wire::VERSION
            )));
        }
        let password = options.server_password.as_deref().filter(|p| !p.is_empty());
        if needs_password && password.is_none() {
            return Err(Failure::new("Dieser Server verlangt ein Passwort.".into()));
        }

        let identity = &self.shared.config.identity;
        let hello = Message::Hello {
            nickname: options.nickname.clone(),
            public_key: identity.public_key().to_owned(),
            signature: identity.sign(&hello_payload(&nonce, host_fingerprint, &options.nickname)),
            password_proof: password
                .filter(|_| needs_password)
                .map(|p| PasswordProof::compute(&nonce, p)),
            default_channel: options.default_channel.unwrap_or(0),
            default_channel_password: options.default_channel_password.clone(),
            input_muted: false,
            output_muted: false,
        };
        write_message(
            send,
            &Envelope {
                request_id: 1,
                message: hello,
            },
        )
        .await
        .map_err(protocol)?;
        match answer(recv).await? {
            welcome @ Message::Welcome { .. } => Ok(welcome),
            _ => Err(Failure::new("Welcome erwartet.".into())),
        }
    }

    /// Nach dem `Welcome`: Stand uebernehmen, Schleifen starten, Verlauf
    /// nachholen.
    fn start(
        &self,
        connection: quinn::Connection,
        endpoint: quinn::Endpoint,
        send: quinn::SendStream,
        recv: quinn::RecvStream,
        welcome: Message,
    ) {
        let server_name = self.apply_welcome(welcome);
        self.open_mirror();
        let (outbox, outgoing) = mpsc::unbounded_channel();
        let live = Arc::new(Live {
            connection,
            endpoint,
            runtime: tokio::runtime::Handle::current(),
            outbox,
            pending: Mutex::new(HashMap::new()),
            next_request: AtomicU32::new(10),
            cancel: CancellationToken::new(),
            kicked: Mutex::new(None),
            started: Instant::now(),
            voice_in_flight: Arc::new(AtomicU32::new(0)),
        });
        *self.shared.live.lock() = Some(Arc::clone(&live));
        self.set_state(ConnectionState::Connected);
        self.publish(EventKind::Connected { server_name });

        let session = self.clone();
        tokio::spawn(async move { session.run_loops(live, send, recv, outgoing).await });

        let session = self.clone();
        tokio::spawn(async move {
            session.sync(crate::chat::ChatArchive::server()).await;
            if let Some(own) = session.snapshot().own_channel() {
                session
                    .sync(crate::chat::ChatArchive::channel(own.id))
                    .await;
            }
        });
    }

    async fn run_loops(
        &self,
        live: Arc<Live>,
        send: quinn::SendStream,
        recv: quinn::RecvStream,
        outgoing: mpsc::UnboundedReceiver<Vec<u8>>,
    ) {
        self.shared.round_trip.reset();
        let reason = tokio::select! {
            () = live.cancel.cancelled() => None,
            reason = write_loop(send, outgoing) => Some(reason),
            reason = self.read_loop(recv) => Some(reason),
            reason = self.datagram_loop(&live) => Some(reason),
            reason = self.voice_stream_loop(&live) => Some(reason),
            reason = self.ping_loop() => Some(reason),
        };
        // Ein `disconnect` hat schon aufgeraeumt.
        let Some(reason) = reason else {
            return;
        };
        let still_ours = {
            let mut current = self.shared.live.lock();
            let ours = current.as_ref().is_some_and(|l| Arc::ptr_eq(l, &live));
            if ours {
                *current = None;
            }
            ours
        };
        if !still_ours {
            return;
        }
        live.connection.close(0u32.into(), b"");
        let kicked = live.kicked.lock().clone();
        self.set_state(ConnectionState::Disconnected);
        self.publish(EventKind::Disconnected {
            was_error: kicked.is_none(),
            reason: kicked.unwrap_or(reason),
        });
    }

    async fn read_loop(&self, mut recv: quinn::RecvStream) -> String {
        loop {
            match read_message(&mut recv).await {
                Ok(Some(envelope)) => self.handle(envelope),
                Ok(None) => return "Verbindung getrennt.".into(),
                Err(error) => return format!("Verbindung verloren: {error}"),
            }
        }
    }

    async fn datagram_loop(&self, live: &Live) -> String {
        loop {
            match live.connection.read_datagram().await {
                Ok(datagram) => self.deliver_voice(&datagram),
                Err(error) => return format!("Verbindung verloren: {error}"),
            }
        }
    }

    /// Sprache als eigener Stream je Frame, wenn Datagramme nicht gehen.
    async fn voice_stream_loop(&self, live: &Live) -> String {
        loop {
            match live.connection.accept_uni().await {
                Ok(mut stream) => {
                    let session = self.clone();
                    tokio::spawn(async move {
                        if let Ok(bytes) = stream.read_to_end(MAX_VOICE_PACKET).await {
                            session.deliver_voice(&bytes);
                        }
                    });
                }
                Err(error) => return format!("Verbindung verloren: {error}"),
            }
        }
    }

    async fn ping_loop(&self) -> String {
        loop {
            let sent = Instant::now();
            match self.ask(Message::Ping).await {
                Ok(Message::Pong) => self.shared.round_trip.add(sent.elapsed()),
                _ => return "Keine Antwort auf Ping.".into(),
            }
            self.changed();
            tokio::time::sleep(PING_INTERVAL).await;
        }
    }

    fn deliver_voice(&self, packet: &[u8]) {
        let Ok(frame) = VoiceFrame::parse(packet) else {
            return;
        };
        let sink = self.shared.on_voice.lock().clone();
        if let Some(sink) = sink {
            sink(ReceivedVoice {
                kind: frame.kind,
                sender: frame.sender,
                sequence: frame.sequence,
                timestamp_ms: frame.timestamp_ms,
                opus: frame.opus.to_vec(),
            });
        }
    }

    // --- Sprache --------------------------------------------------------------

    /// Ein kodierter Opus-Frame (20 ms) an den eigenen Kanal; leer = Ende des
    /// Sprechens. Synchron und thread-sicher, vom Aufnahme-Thread gerufen.
    /// Als Datagramm, sonst ein Frame je unidirektionalem Stream.
    pub fn send_voice(&self, opus: &[u8]) {
        let Some(live) = self.shared.live.lock().clone() else {
            return;
        };
        let elapsed = u32::try_from(live.started.elapsed().as_millis()).unwrap_or(u32::MAX);
        let frame = VoiceFrame {
            kind: FrameKind::Voice,
            sender: 0,
            sequence: self.shared.voice_sequence.fetch_add(1, Ordering::Relaxed),
            timestamp_ms: elapsed,
            opus,
        };
        let bytes = Bytes::from(frame.to_bytes());
        match live.connection.send_datagram(bytes.clone()) {
            Ok(()) | Err(quinn::SendDatagramError::ConnectionLost(_)) => {}
            Err(_) => send_voice_stream(&live, bytes),
        }
    }

    // --- Intern ---------------------------------------------------------------

    fn set_state(&self, state: ConnectionState) {
        let value = match state {
            ConnectionState::Disconnected => 0,
            ConnectionState::Connecting => 1,
            ConnectionState::Connected => 2,
        };
        self.shared.state.store(value, Ordering::Release);
        self.changed();
    }

    fn publish(&self, kind: EventKind) {
        self.shared.events.lock().push_back(SessionEvent::now(kind));
        self.changed();
    }

    fn changed(&self) {
        let callback = self.shared.on_change.lock().clone();
        if let Some(callback) = callback {
            callback();
        }
    }

    fn apply_welcome(&self, welcome: Message) -> String {
        let Message::Welcome {
            client_id,
            server_name,
            welcome_message,
            channels,
            clients,
            streams,
            ..
        } = welcome
        else {
            return String::new();
        };
        {
            let mut model = self.shared.model.lock();
            *model = Model {
                server_name: server_name.clone(),
                welcome_message,
                own_id: client_id,
                channels: channels.into_iter().map(|c| (c.id, c)).collect(),
                clients: clients.into_iter().map(|c| (c.id, c)).collect(),
                streams: streams.into_iter().map(|s| (s.id, s)).collect(),
            };
        }
        self.publish_snapshot();
        server_name
    }

    fn mutate(&self, change: impl FnOnce(&mut Model)) {
        change(&mut self.shared.model.lock());
        self.publish_snapshot();
    }

    fn publish_snapshot(&self) {
        let snapshot = {
            let model = self.shared.model.lock();
            let channels: Vec<_> = model.channels.values().cloned().collect();
            let clients: Vec<_> = model.clients.values().cloned().collect();
            let streams: Vec<_> = model.streams.values().cloned().collect();
            ServerSnapshot::build(
                &model.server_name,
                &model.welcome_message,
                model.own_id,
                &channels,
                &clients,
                &streams,
                self.shared.version.fetch_add(1, Ordering::Relaxed) + 1,
            )
        };
        self.shared.snapshot.store(Arc::new(snapshot));
        self.changed();
    }

    fn open_mirror(&self) {
        let host = self.host_fingerprint();
        let mirror = self
            .shared
            .config
            .mirror_root
            .as_ref()
            .zip(file_key(&host))
            .map(|(root, key)| Arc::new(ChatMirror::new(root.join(key))));
        *self.shared.mirror.lock() = mirror;
    }

    fn mirror(&self) -> Option<Arc<ChatMirror>> {
        self.shared.mirror.lock().clone()
    }

    fn live(&self) -> Option<Arc<Live>> {
        self.shared.live.lock().clone()
    }

    fn own_fingerprint(&self) -> &str {
        self.shared.config.identity.fingerprint()
    }
}

/// Warum ein Verbinden scheiterte.
struct Failure {
    reason: String,
    /// Geboxt: das Ereignis ist gross, der Fehlerweg soll es nicht sein.
    host_key_changed: Option<Box<EventKind>>,
}

impl Failure {
    fn new(reason: String) -> Self {
        Self {
            reason,
            host_key_changed: None,
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn protocol(error: echo_protocol::ProtocolError) -> Failure {
    Failure::new(format!("Leitung: {error}"))
}

/// Eine Antwort im Handschlag; eine Ablehnung wird zum Fehler.
async fn answer(recv: &mut quinn::RecvStream) -> Result<Message, Failure> {
    let envelope = tokio::time::timeout(REQUEST_TIMEOUT, read_message(recv))
        .await
        .map_err(|_| Failure::new("Der Server antwortet nicht.".into()))?
        .map_err(protocol)?
        .ok_or_else(|| Failure::new("Der Server hat aufgelegt.".into()))?;
    match envelope.message {
        Message::Reply { ok: false, error } => {
            Err(Failure::new(error.unwrap_or_else(|| "Abgelehnt.".into())))
        }
        message => Ok(message),
    }
}

async fn resolve(host: &str, port: u16) -> Result<SocketAddr, Failure> {
    tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| Failure::new(format!("{host} ist nicht zu finden: {e}")))?
        .next()
        .ok_or_else(|| Failure::new(format!("{host} ist nicht zu finden.")))
}

/// Ein eigener Endpunkt je Verbindung, in der Adressfamilie des Ziels.
fn client_endpoint(target: SocketAddr) -> Result<quinn::Endpoint, Failure> {
    let bind: SocketAddr = if target.is_ipv4() {
        (Ipv4Addr::UNSPECIFIED, 0).into()
    } else {
        (Ipv6Addr::UNSPECIFIED, 0).into()
    };
    let socket = std::net::UdpSocket::bind(bind).map_err(|e| Failure::new(format!("Netz: {e}")))?;
    let endpoint_config = echo_quic::endpoint_config().map_err(|e| Failure::new(e.to_string()))?;
    let mut endpoint =
        quinn::Endpoint::new(endpoint_config, None, socket, Arc::new(quinn::TokioRuntime))
            .map_err(|e| Failure::new(format!("Netz: {e}")))?;
    let mut config =
        echo_quic::client_config(Wire::ALPN).map_err(|e| Failure::new(e.to_string()))?;
    let mut transport = quinn::TransportConfig::default();
    transport.max_idle_timeout(quinn::IdleTimeout::try_from(IDLE_TIMEOUT).ok());
    config.transport_config(Arc::new(transport));
    endpoint.set_default_client_config(config);
    Ok(endpoint)
}

async fn write_loop(
    mut send: quinn::SendStream,
    mut outgoing: mpsc::UnboundedReceiver<Vec<u8>>,
) -> String {
    while let Some(frame) = outgoing.recv().await {
        if let Err(error) = send.write_all(&frame).await {
            return format!("Verbindung verloren: {error}");
        }
    }
    "Verbindung getrennt.".into()
}

fn send_voice_stream(live: &Arc<Live>, frame: Bytes) {
    if live.voice_in_flight.fetch_add(1, Ordering::AcqRel) >= MAX_VOICE_IN_FLIGHT {
        live.voice_in_flight.fetch_sub(1, Ordering::AcqRel);
        return;
    }
    let connection = live.connection.clone();
    let in_flight = Arc::clone(&live.voice_in_flight);
    live.runtime.spawn(async move {
        // Verbindung weg oder ueberlastet: der Frame ist verloren, mehr nicht.
        if let Ok(mut stream) = connection.open_uni().await {
            let _ = stream.write_all(&frame).await;
            let _ = stream.finish();
        }
        in_flight.fetch_sub(1, Ordering::AcqRel);
    });
}

/// Kodiert eine Nachricht fuer die Sendequeue.
fn frame(envelope: &Envelope) -> Option<Vec<u8>> {
    encode(envelope).ok()
}
