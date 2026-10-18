use std::path::PathBuf;

/// Was beim Zugriff auf ein Archiv schiefgehen kann.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Datei nicht erreichbar: {0}")]
    Io(#[from] std::io::Error),

    #[error("SQLite: {0}")]
    Engine(#[from] turso_core::LimboError),

    /// turso nimmt Pfade nur als Text.
    #[error("Pfad ist kein gueltiges Unicode: {0}")]
    PathNotUnicode(PathBuf),
}
