//! Speicher von echo: sqlar-Archive des Servers und (spaeter) die
//! JSONL-Dateien des Clients. Die Formate auf der Platte sind die des
//! C#-Vorgaengers, damit ein umgestellter Server seine Daten behaelt
//! (README, Abschnitt „turso und die sqlar-Archive“).
//!
//! Kennt weder Netz noch UI.

mod archive_keys;
mod database;
mod error;

pub use archive_keys::{file_key, fingerprint};
pub use database::{Database, Row, Value};
pub use error::StorageError;
