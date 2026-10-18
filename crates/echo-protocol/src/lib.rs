//! Das Protokoll von echo: was zwischen Client und Server ueber die Leitung
//! geht, und wer was darf. Kennt kein Netz und keine Sockets; das Framing
//! liest und schreibt nur ueber `AsyncRead`/`AsyncWrite`.
//!
//! Gegenueber dem C#-Vorgaenger fehlen `DatagramOffer` und die zweite
//! Verbindung fuer Datagramme: die gab es nur, weil System.Net.Quic keine
//! Datagramme durchreicht. Mit quinn gehen Sprachframes als Datagramm ueber
//! die Steuerverbindung (echo-231).

mod error;
mod framing;
mod identity;
mod image_formats;
mod messages;
mod rights;
mod voice_frame;
mod wire;

pub use error::ProtocolError;
pub use framing::{decode, encode, read_message, write_message};
pub use identity::{Identity, PasswordProof, hello_payload, new_nonce};
pub use image_formats::ImageFormat;
pub use messages::{
    ApprovalDto, ApprovalState, ArchiveRef, BoardDto, ChannelDto, ChatImageDto, ChatMessageDto,
    ChatScope, ClientDto, DmSummary, Envelope, FileEntryDto, LeaveReason, Message, Role, ShapeDto,
    StreamDto, StreamKind,
};
pub use rights::Rights;
pub use voice_frame::{FrameKind, VoiceFrame};
pub use wire::Wire;
