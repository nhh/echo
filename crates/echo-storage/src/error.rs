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

    #[error("Spalte {index} ist kein {expected}")]
    Column {
        index: usize,
        expected: &'static str,
    },

    /// Namen kommen aus Kennungen des Programms; dieser zeigte aus dem Archiv
    /// heraus oder in den Bereich der Stuecke.
    #[error("ungueltiger Archivname „{0}“")]
    InvalidName(String),

    /// Fremd geschriebene, gepackte Zeilen lassen sich nicht an beliebiger
    /// Stelle lesen.
    #[error("„{0}“ liegt gepackt im Archiv und laesst sich nicht streamen")]
    Compressed(String),

    #[error("Inhalt fuer „{name}“ endet nach {written} von {length} Byte")]
    ShortContent {
        name: String,
        written: u64,
        length: u64,
    },

    #[error("„{0}“ passt nicht zum Stand der Uebernahme")]
    MoveMismatch(PathBuf),

    #[error("abgebrochen")]
    Canceled,
}
