//! Ein Server im Prozess und ein schlanker Client, der wie der echte
//! Client ueber QUIC spricht: Handschlag, Anfragen, Ereignisse, Sprache.

// Hilfsfunktionen ausserhalb von #[test] zaehlen fuer clippy nicht als Test.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    dead_code,
    unreachable_pub
)]

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use echo_protocol::{
    Envelope, FrameKind, Identity, Message, PasswordProof, VoiceFrame, Wire, hello_payload,
    read_message, write_message,
};
use echo_server::{EchoServer, ServerOptions, ServerPaths, ServerSettings};
use quinn::{Endpoint, TokioRuntime};
use tempfile::TempDir;

pub const TIMEOUT: Duration = Duration::from_secs(10);

pub struct TestServer {
    pub server: EchoServer,
    pub dir: TempDir,
}

impl TestServer {
    pub fn start(settings: ServerSettings) -> Self {
        let dir = TempDir::new().unwrap();
        let server = Self::start_in(dir.path(), Some(settings));
        Self { server, dir }
    }

    pub fn start_in(data: &Path, settings: Option<ServerSettings>) -> EchoServer {
        let paths = ServerPaths::new(Some(data.to_owned())).unwrap();
        let options = ServerOptions {
            bind: Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))),
            idle_timeout: Duration::from_secs(5),
        };
        EchoServer::start(&paths, settings, &options, Arc::new(|_| {})).unwrap()
    }

    pub fn address(&self) -> SocketAddr {
        self.server.local_addr().unwrap()
    }

    pub fn host(&self) -> String {
        self.server.host_fingerprint().to_owned()
    }
}

pub fn settings() -> ServerSettings {
    ServerSettings {
        name: "Test".into(),
        ..ServerSettings::default()
    }
}

/// Was ein Client beim Verbinden mitbringt.
pub struct Login<'a> {
    pub nickname: &'a str,
    pub password: Option<&'a str>,
    pub identity: Option<Identity>,
}

impl<'a> Login<'a> {
    pub fn named(nickname: &'a str) -> Self {
        Self {
            nickname,
            password: None,
            identity: None,
        }
    }
}

pub struct TestClient {
    pub endpoint: Endpoint,
    pub connection: quinn::Connection,
    send: quinn::SendStream,
    recv: quinn::RecvStream,
    pub id: u16,
    pub welcome: Message,
    pub identity: Identity,
    next_request: u32,
    /// Ereignisse, die beim Warten auf eine Antwort ankamen.
    pending: Vec<Message>,
}

pub fn client_endpoint() -> Endpoint {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let mut endpoint = Endpoint::new(
        echo_quic::endpoint_config().unwrap(),
        None,
        socket,
        Arc::new(TokioRuntime),
    )
    .unwrap();
    endpoint.set_default_client_config(echo_quic::client_config(Wire::ALPN).unwrap());
    endpoint
}

/// Verbindet und schickt `Open`; liefert Verbindung und Steuerstream.
pub async fn open(
    address: SocketAddr,
    version: u32,
) -> (
    Endpoint,
    quinn::Connection,
    quinn::SendStream,
    quinn::RecvStream,
) {
    let endpoint = client_endpoint();
    let connection = endpoint.connect(address, "echo").unwrap().await.unwrap();
    let (mut send, recv) = connection.open_bi().await.unwrap();
    write_message(
        &mut send,
        &Envelope {
            request_id: 1,
            message: Message::Open { version },
        },
    )
    .await
    .unwrap();
    (endpoint, connection, send, recv)
}

pub async fn read(recv: &mut quinn::RecvStream) -> Envelope {
    tokio::time::timeout(TIMEOUT, read_message(recv))
        .await
        .expect("Antwort kam nicht")
        .unwrap()
        .expect("Stream endete")
}

/// Der ganze Handschlag; `Err` mit dem Grund, wenn der Server ablehnt.
pub async fn connect(server: &TestServer, login: Login<'_>) -> Result<TestClient, String> {
    connect_to(server.address(), &server.host(), login).await
}

pub async fn connect_to(
    address: SocketAddr,
    host: &str,
    login: Login<'_>,
) -> Result<TestClient, String> {
    let (endpoint, connection, mut send, mut recv) = open(address, Wire::VERSION).await;
    assert_eq!(
        echo_quic::server_fingerprint(&connection).as_deref(),
        Some(host)
    );

    let Message::Challenge { nonce, .. } = read(&mut recv).await.message else {
        panic!("Challenge erwartet");
    };
    let identity = login
        .identity
        .unwrap_or_else(|| Identity::generate().unwrap());
    let hello = Message::Hello {
        nickname: login.nickname.into(),
        public_key: identity.public_key().into(),
        signature: identity.sign(&hello_payload(&nonce, host, login.nickname)),
        password_proof: login.password.map(|p| PasswordProof::compute(&nonce, p)),
        default_channel: 0,
        default_channel_password: None,
        input_muted: false,
        output_muted: false,
    };
    write_message(
        &mut send,
        &Envelope {
            request_id: 2,
            message: hello,
        },
    )
    .await
    .unwrap();

    match read(&mut recv).await.message {
        welcome @ Message::Welcome { client_id, .. } => Ok(TestClient {
            endpoint,
            connection,
            send,
            recv,
            id: client_id,
            welcome,
            identity,
            next_request: 10,
            pending: Vec::new(),
        }),
        Message::Reply { ok: false, error } => Err(error.unwrap_or_default()),
        other => panic!("unerwartet: {other:?}"),
    }
}

impl TestClient {
    pub fn role(&self) -> echo_protocol::Role {
        match &self.welcome {
            Message::Welcome { role, .. } => *role,
            _ => unreachable!(),
        }
    }

    pub fn channel_names(&self) -> Vec<String> {
        match &self.welcome {
            Message::Welcome { channels, .. } => channels.iter().map(|c| c.name.clone()).collect(),
            _ => unreachable!(),
        }
    }

    /// Schickt eine Anfrage und wartet auf ihre Antwort; Ereignisse
    /// dazwischen bleiben fuer `event` liegen.
    pub async fn request(&mut self, message: Message) -> Message {
        let id = self.next_request;
        self.next_request += 1;
        write_message(
            &mut self.send,
            &Envelope {
                request_id: id,
                message,
            },
        )
        .await
        .unwrap();
        loop {
            let envelope = read(&mut self.recv).await;
            if envelope.request_id == id {
                return envelope.message;
            }
            self.pending.push(envelope.message);
        }
    }

    /// `Ok`, wenn der Server mit `Reply { ok: true }` antwortet, sonst der Grund.
    pub async fn ask(&mut self, message: Message) -> Result<(), String> {
        match self.request(message).await {
            Message::Reply { ok: true, .. } => Ok(()),
            Message::Reply { ok: false, error } => Err(error.unwrap_or_default()),
            other => panic!("Reply erwartet: {other:?}"),
        }
    }

    /// Ein Ereignis ohne Antwort zu erwarten.
    pub async fn tell(&mut self, message: Message) {
        write_message(&mut self.send, &Envelope::event(message))
            .await
            .unwrap();
    }

    /// Wartet auf das erste Ereignis, das `wanted` erfuellt.
    pub async fn event(&mut self, wanted: impl Fn(&Message) -> bool) -> Message {
        if let Some(index) = self.pending.iter().position(&wanted) {
            return self.pending.remove(index);
        }
        loop {
            let message = read(&mut self.recv).await.message;
            if wanted(&message) {
                return message;
            }
        }
    }

    pub fn send_voice(&self, sequence: u16, opus: &[u8]) {
        let frame = VoiceFrame {
            kind: FrameKind::Voice,
            sender: 0,
            sequence,
            timestamp_ms: 0,
            opus,
        };
        self.connection
            .send_datagram(frame.to_bytes().into())
            .unwrap();
    }

    /// Der naechste Sprachframe, als Datagramm oder als Stream.
    pub async fn voice(&self, within: Duration) -> Option<Vec<u8>> {
        tokio::time::timeout(within, async {
            tokio::select! {
                datagram = self.connection.read_datagram() => datagram.ok().map(|d| d.to_vec()),
                stream = self.connection.accept_uni() => {
                    let mut stream = stream.ok()?;
                    stream.read_to_end(2000).await.ok()
                }
            }
        })
        .await
        .ok()
        .flatten()
    }
}

/// Die Datenverbindung: eigene QUIC-Verbindung mit `TransferHello`. Liefert
/// die erste Antwort des Servers und die Streams fuer die Bytes.
pub async fn data_connection(
    address: SocketAddr,
    token: &str,
) -> (
    Endpoint,
    quinn::Connection,
    quinn::SendStream,
    quinn::RecvStream,
    Result<(), String>,
) {
    let endpoint = client_endpoint();
    let connection = endpoint.connect(address, "echo").unwrap().await.unwrap();
    let (mut send, mut recv) = connection.open_bi().await.unwrap();
    let hello = Envelope {
        request_id: 1,
        message: Message::TransferHello {
            token: token.into(),
        },
    };
    write_message(&mut send, &hello).await.unwrap();
    let answer = match read(&mut recv).await.message {
        Message::Reply { ok: true, .. } => Ok(()),
        Message::Reply { ok: false, error } => Err(error.unwrap_or_default()),
        other => panic!("Reply erwartet: {other:?}"),
    };
    (endpoint, connection, send, recv, answer)
}

/// Laedt `bytes` mit dem Token hoch; `Err` mit dem Grund des Servers.
pub async fn upload(address: SocketAddr, token: &str, bytes: &[u8]) -> Result<(), String> {
    let (_endpoint, _connection, mut send, mut recv, opened) =
        data_connection(address, token).await;
    opened?;
    send.write_all(bytes).await.unwrap();
    let finished = match read(&mut recv).await.message {
        Message::Reply { ok: true, .. } => Ok(()),
        Message::Reply { ok: false, error } => Err(error.unwrap_or_default()),
        other => panic!("Reply erwartet: {other:?}"),
    };
    send.finish().unwrap();
    finished
}

/// Laedt mit dem Token herunter.
pub async fn download(address: SocketAddr, token: &str) -> Result<Vec<u8>, String> {
    let (_endpoint, _connection, mut send, mut recv, opened) =
        data_connection(address, token).await;
    opened?;
    let bytes = tokio::time::timeout(TIMEOUT, recv.read_to_end(usize::MAX))
        .await
        .expect("Download stockt")
        .unwrap();
    send.finish().unwrap();
    Ok(bytes)
}

/// Das Token aus einem `TransferTicket`.
pub fn token(message: Message) -> String {
    match message {
        Message::TransferTicket { token, .. } => token,
        other => panic!("TransferTicket erwartet: {other:?}"),
    }
}
