//! Ein Server im Prozess und Sessions, die mit ihm sprechen.

// Hilfsfunktionen ausserhalb von #[test] zaehlen fuer clippy nicht als Test.
#![allow(clippy::unwrap_used, clippy::panic, dead_code, unreachable_pub)]

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use echo_core::{ConnectOptions, EventKind, Session, SessionConfig};
use echo_protocol::Identity;
use echo_server::{EchoServer, ServerOptions, ServerPaths, ServerSettings};
use echo_storage::ClientDatabase;
use tempfile::TempDir;

pub const TIMEOUT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(5);

pub struct TestServer {
    pub server: EchoServer,
    pub dir: TempDir,
}

impl TestServer {
    pub fn start(settings: ServerSettings) -> Self {
        let dir = TempDir::new().unwrap();
        let paths = ServerPaths::new(Some(dir.path().to_owned())).unwrap();
        let options = ServerOptions {
            bind: Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))),
            idle_timeout: Duration::from_secs(5),
        };
        let server = EchoServer::start(&paths, Some(settings), &options, Arc::new(|_| {})).unwrap();
        Self { server, dir }
    }

    pub fn address(&self) -> String {
        self.server.local_addr().unwrap().to_string()
    }
}

pub fn settings() -> ServerSettings {
    ServerSettings {
        name: "Test".into(),
        ..ServerSettings::default()
    }
}

/// Ein Client mit eigener Identitaet, eigener Datenbank und eigenem Spiegel.
pub struct TestClient {
    pub session: Session,
    pub dir: TempDir,
    pub identity: Arc<Identity>,
}

impl TestClient {
    pub fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let identity = Arc::new(Identity::generate().unwrap());
        let session = Self::session_in(&dir, &identity);
        Self {
            session,
            dir,
            identity,
        }
    }

    /// Eine neue Session auf denselben Daten, wie nach einem Neustart.
    pub fn restart(&self) -> Session {
        Self::session_in(&self.dir, &self.identity)
    }

    fn session_in(dir: &TempDir, identity: &Arc<Identity>) -> Session {
        let db = ClientDatabase::open(&dir.path().join("client.sqlite")).unwrap();
        Session::new(SessionConfig {
            identity: Arc::clone(identity),
            client_db: Arc::new(db),
            mirror_root: Some(dir.path().join("chats")),
        })
    }

    pub fn mirror(&self) -> PathBuf {
        self.dir.path().join("chats")
    }

    pub async fn connect(server: &TestServer, nickname: &str) -> Self {
        let client = Self::new();
        client
            .session
            .connect(ConnectOptions::new(&server.address(), nickname))
            .await
            .unwrap();
        client
    }
}

/// Wartet auf das erste Ereignis, das `pick` annimmt; alles davor verfaellt.
pub async fn event<T>(session: &Session, mut pick: impl FnMut(&EventKind) -> Option<T>) -> T {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        while let Some(event) = session.next_event() {
            if let Some(found) = pick(&event.kind) {
                return found;
            }
        }
        assert!(Instant::now() < deadline, "Ereignis kam nicht");
        tokio::time::sleep(POLL).await;
    }
}

/// Wartet, bis der Schnappschuss `ready` erfuellt.
pub async fn until(session: &Session, ready: impl Fn(&echo_core::ServerSnapshot) -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    while !ready(&session.snapshot()) {
        assert!(Instant::now() < deadline, "Zustand kam nicht");
        tokio::time::sleep(POLL).await;
    }
}

/// Wartet auf eine Chatzeile mit diesem Text.
pub async fn chat(session: &Session, text: &str) -> echo_core::ChatLine {
    event(session, |kind| match kind {
        EventKind::Chat(line) if line.text == text => Some(line.clone()),
        _ => None,
    })
    .await
}
