//! Speicher von echo: die sqlar-Archive des Servers, im Format des
//! C#-Vorgaengers, damit ein umgestellter Server seine Daten behaelt
//! (`docs/storage.md`), und die SQLite-Datei des Clients.
//!
//! Kennt weder Netz noch UI.

mod archive_keys;
mod database;
mod error;

pub use archive_keys::{file_key, fingerprint};
pub use database::{Database, Row, Value};
pub use error::StorageError;
