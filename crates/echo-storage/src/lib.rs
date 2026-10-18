//! Speicher von echo: die sqlar-Archive des Servers, im Format des
//! C#-Vorgaengers, damit ein umgestellter Server seine Daten behaelt
//! (`docs/storage.md`), und die SQLite-Datei des Clients.
//!
//! Kennt weder Netz noch UI.

mod archive_keys;
mod content;
mod database;
mod error;
mod sqlar;

pub use archive_keys::{file_key, fingerprint};
pub use content::ContentReader;
pub use database::{Connection, Database, Row, Value, param};
pub use error::StorageError;
pub use sqlar::SqlarArchive;
