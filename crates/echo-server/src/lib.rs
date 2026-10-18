//! Der Server von echo: QUIC mit eigenem TLS-Provider, Kanaele, Chat,
//! Moderation, Sprache als Datagramm (SFU: der Server dekodiert nie).
//! Daten und Konfiguration liegen in den sqlar-Archiven des C#-Vorgaengers.
//!
//! Kern (echo-232) sowie Dateien, Datenverbindung und Chatbilder (echo-235)
//! stehen; Tafeln und Medien-Streams folgen.

mod archives;
mod channels;
mod client;
mod clients;
mod connection;
mod files;
mod paths;
mod server;
mod state;
mod stores;
mod ticks;
mod transfer;
mod transfers;

pub use connection::SERVER_VERSION;
pub use paths::ServerPaths;
pub use server::{DEFAULT_IDLE_TIMEOUT, EchoServer, Log, ServerError, ServerOptions};
pub use stores::ServerSettings;
