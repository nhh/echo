//! Der Server: laedt Datenordner und Host-Zertifikat, hoert auf QUIC und
//! fuehrt je Verbindung einen Client. Auch in-process nutzbar (Port 0 =
//! beliebig), so testen die Tests ihn.

use std::net::{Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use echo_protocol::Wire;
use echo_quic::HostCertificate;
use echo_storage::{ChannelArchive, StorageError};
use quinn::{Endpoint, IdleTimeout, TokioRuntime, TransportConfig, VarInt};
use socket2::{Domain, Protocol, Socket, Type};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::archives::ArchiveSet;
use crate::connection;
use crate::paths::ServerPaths;
use crate::state::ServerState;
use crate::stores::{ServerDatabase, ServerSettings};

/// Wie lange eine Verbindung ohne jedes Paket bestehen bleibt.
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(45);

/// So lange duerfen Verbindungen beim Beenden noch aufraeumen.
const STOP_GRACE: Duration = Duration::from_secs(3);

/// Fuer Dateien und Video (echo-176): ein Stream traegt hoechstens ein Fenster
/// je Laufzeit; mit 16 MB sind es bei 150 ms gut 100 MB/s. Belegt wird nur,
/// was noch nicht gelesen ist, und gelesen wird sofort.
const STREAM_WINDOW: u32 = 16 * 1024 * 1024;
const CONNECTION_WINDOW: u32 = 32 * 1024 * 1024;

/// Je Sprachframe ein Stream, wenn Datagramme nicht gehen.
const MAX_UNI_STREAMS: u32 = 4096;
const MAX_BI_STREAMS: u32 = 4;

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("Speicher: {0}")]
    Storage(#[from] StorageError),
    #[error("TLS/QUIC: {0}")]
    Config(#[from] echo_quic::ConfigError),
    #[error("Netz: {0}")]
    Io(#[from] std::io::Error),
}

/// Wie der Server laufen soll; die Vorgaben passen fuer den Betrieb.
#[derive(Debug, Clone)]
pub struct ServerOptions {
    /// `None`: alle Adressen beider Familien auf dem Port aus den Einstellungen.
    pub bind: Option<SocketAddr>,
    pub idle_timeout: Duration,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self {
            bind: None,
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
        }
    }
}

pub type Log = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Debug)]
pub struct EchoServer {
    endpoint: Endpoint,
    state: Arc<ServerState>,
    host_fingerprint: Arc<str>,
    settings: ServerSettings,
    stopping: CancellationToken,
    tasks: TaskTracker,
}

impl EchoServer {
    /// Oeffnet den Datenordner und hoert sofort. `settings` ersetzt die
    /// gespeicherten Einstellungen fuer diesen Lauf (Tests); ohne sie gelten
    /// die gespeicherten, und ein frischer Server speichert die Vorgaben.
    pub fn start(
        paths: &ServerPaths,
        settings: Option<ServerSettings>,
        options: &ServerOptions,
        log: Log,
    ) -> Result<Self, ServerError> {
        let archive = Arc::new(ChannelArchive::open(&paths.server_archive())?);
        let database = ServerDatabase::open(Arc::clone(&archive))?;
        let settings = match settings {
            Some(settings) => settings,
            None => saved_or_default(&database)?,
        };

        let certificate = HostCertificate::load_or_generate(
            &paths.host_certificate(),
            &paths.host_key(),
            &settings.name,
        )?;
        let host_fingerprint: Arc<str> = certificate.fingerprint().into();
        let archives = Arc::new(ArchiveSet::new(paths.clone(), archive)?);
        let state_log = Arc::clone(&log);
        let state = Arc::new(ServerState::new(
            settings.clone(),
            database,
            archives,
            Box::new(move |line| state_log(line)),
        )?);

        let mut server_config = echo_quic::server_config(&certificate, Wire::ALPN)?;
        server_config.transport_config(Arc::new(transport(options.idle_timeout)));
        let bind = options
            .bind
            .unwrap_or_else(|| SocketAddr::from((Ipv6Addr::UNSPECIFIED, settings.port)));
        let endpoint = Endpoint::new(
            echo_quic::endpoint_config()?,
            Some(server_config),
            bind_socket(bind)?,
            Arc::new(TokioRuntime),
        )?;

        let server = Self {
            endpoint,
            state,
            host_fingerprint,
            settings,
            stopping: CancellationToken::new(),
            tasks: TaskTracker::new(),
        };
        log(&format!(
            "echo-server '{}' hört auf {} (Host {}), Daten in {}",
            server.settings.name,
            server.local_addr()?,
            server.host_fingerprint,
            paths.root().display()
        ));
        server.accept_loop(log);
        Ok(server)
    }

    /// Aendert die gespeicherten Einstellungen, ohne den Server zu starten
    /// (Kommandozeile), und gibt sie zurueck.
    pub fn update_settings(
        paths: &ServerPaths,
        change: impl FnOnce(&mut ServerSettings),
    ) -> Result<ServerSettings, ServerError> {
        let archive = Arc::new(ChannelArchive::open(&paths.server_archive())?);
        let database = ServerDatabase::open(archive)?;
        let mut settings = database.settings()?.unwrap_or_default();
        change(&mut settings);
        database.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.endpoint.local_addr()
    }

    #[must_use]
    pub fn host_fingerprint(&self) -> &str {
        &self.host_fingerprint
    }

    /// Wie viele Clients gerade verbunden sind.
    #[must_use]
    pub fn client_count(&self) -> usize {
        self.state.client_count()
    }

    #[must_use]
    pub fn settings(&self) -> &ServerSettings {
        &self.settings
    }

    fn accept_loop(&self, log: Log) {
        let endpoint = self.endpoint.clone();
        let state = Arc::clone(&self.state);
        let host = Arc::clone(&self.host_fingerprint);
        let stopping = self.stopping.clone();
        let tasks = self.tasks.clone();
        self.tasks.spawn(async move {
            loop {
                let incoming = tokio::select! {
                    () = stopping.cancelled() => return,
                    incoming = endpoint.accept() => incoming,
                };
                let Some(incoming) = incoming else {
                    return;
                };
                let (state, host, stopping, log) = (
                    Arc::clone(&state),
                    Arc::clone(&host),
                    stopping.clone(),
                    Arc::clone(&log),
                );
                tasks.spawn(async move {
                    // Ein gescheiterter Handschlag betrifft nur diese eine
                    // Verbindung; die Schleife nimmt weiter an (echo-151).
                    match incoming.await {
                        Ok(connection) => connection::run(connection, state, host, stopping).await,
                        Err(error) => log(&format!("Verbindung abgelehnt: {error}")),
                    }
                });
            }
        });
    }

    /// Trennt alle Clients mit dem Grund "Server gestoppt" und wartet kurz,
    /// bis sie aufgeraeumt haben.
    pub async fn stop(self) {
        self.stopping.cancel();
        self.tasks.close();
        let _ = tokio::time::timeout(STOP_GRACE, self.tasks.wait()).await;
        self.endpoint.close(0u32.into(), b"");
        let _ = tokio::time::timeout(STOP_GRACE, self.endpoint.wait_idle()).await;
    }
}

/// Ein frischer Server speichert die Vorgaben, damit der Betreiber sie findet.
fn saved_or_default(database: &ServerDatabase) -> Result<ServerSettings, ServerError> {
    if let Some(saved) = database.settings()? {
        return Ok(saved);
    }
    let defaults = ServerSettings::default();
    database.save_settings(&defaults)?;
    Ok(defaults)
}

fn transport(idle_timeout: Duration) -> TransportConfig {
    let mut transport = TransportConfig::default();
    transport
        .max_idle_timeout(IdleTimeout::try_from(idle_timeout).ok())
        .max_concurrent_uni_streams(VarInt::from_u32(MAX_UNI_STREAMS))
        .max_concurrent_bidi_streams(VarInt::from_u32(MAX_BI_STREAMS))
        .stream_receive_window(VarInt::from_u32(STREAM_WINDOW))
        .receive_window(VarInt::from_u32(CONNECTION_WINDOW));
    transport
}

/// Auf `[::]` nimmt der Socket auch IPv4 an; nur IPv4 zu binden hiesse, wer
/// ueber IPv6 kommt (auch "localhost" -> ::1), erreicht den Server nicht
/// (echo-145).
fn bind_socket(address: SocketAddr) -> std::io::Result<UdpSocket> {
    let socket = Socket::new(
        Domain::for_address(address),
        Type::DGRAM,
        Some(Protocol::UDP),
    )?;
    if address.is_ipv6() {
        socket.set_only_v6(false)?;
    }
    socket.bind(&address.into())?;
    Ok(socket.into())
}
