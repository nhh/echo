//! Konstanten des Protokolls.

/// Feste Groessen und Kennungen der Leitung.
#[derive(Debug)]
pub struct Wire;

impl Wire {
    /// ALPN-Kennung, bewusst ohne Version: so kommt die Verbindung zustande,
    /// und die Versionen werden im Handschlag verglichen, wo der Server einen
    /// lesbaren Grund senden kann. Mit einer Version in der Kennung lehnte
    /// schon TLS ab, und der Nutzer saehe nur "no application protocol".
    pub const ALPN: &'static [u8] = b"echo";

    /// Steigt bei jeder Aenderung an den Nachrichten, weil postcard wie
    /// MemoryPack nicht versionstolerant ist. Server und Client werden
    /// zusammen ausgeliefert. Der Rust-Port setzt die Zaehlung des
    /// C#-Vorgaengers (21) fort, mit dem er nicht spricht.
    pub const VERSION: u32 = 22;

    pub const DEFAULT_PORT: u16 = 9988;

    /// Adress-Schema, an dem der Client den eigenen Server erkennt.
    pub const SCHEME: &'static str = "echo://";

    pub const MAX_MESSAGE_BYTES: usize = 1 << 20;
    pub const MAX_VOICE_FRAME_BYTES: usize = 1500;

    /// Ein Keyframe bei 720p liegt im Bereich von hunderttausend Byte; eine
    /// Million laesst Luft nach oben (echo-49).
    pub const MAX_VIDEO_FRAME_BYTES: usize = 1_000_000;

    /// Ein langes GIF liegt bei ein paar Megabyte; mehr gehoert in den
    /// Dateibereich (echo-166).
    pub const MAX_CHAT_IMAGE_BYTES: u64 = 16 * 1024 * 1024;

    /// Mehr zeigt kein Bildschirm, und der Empfaenger muss es dekodieren koennen.
    pub const MAX_CHAT_IMAGE_SIDE: u32 = 16384;
}
