/// Was beim Lesen oder Schreiben des Protokolls schiefgehen kann.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("Nachricht zu gross: {0} Byte")]
    TooLarge(usize),

    #[error("ungueltige Nachrichtenlaenge {0}")]
    InvalidLength(u32),

    #[error("Stream endete mitten in einer Nachricht")]
    Truncated,

    #[error("unlesbare Nachricht: {0}")]
    Malformed(#[from] postcard::Error),

    #[error("Leitung: {0}")]
    Io(#[from] std::io::Error),

    #[error("Sprachframe zu kurz: {0} Byte")]
    ShortVoiceFrame(usize),

    #[error("unbekannte Art von Sprachframe: {0}")]
    UnknownFrameKind(u8),

    #[error("Schluessel: {0}")]
    Key(String),
}
