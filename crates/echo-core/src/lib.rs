//! Der Kern des echo-Clients ohne Oberflaeche: die Session zum eigenen
//! Server, der Serverbaum als Schnappschuss, Chat und Verlauf. Kennt weder
//! Fenster noch Grafik; die Oberflaeche liest Schnappschuss und Ereignisse
//! je Frame.

mod address;
mod channel_tree;
mod chat;
mod events;
mod mirror;
mod round_trip;
mod session;
mod snapshot;

pub use address::{is_echo_address, parse_address};
pub use channel_tree::{children_of, clients_in, path_to};
pub use chat::{
    ChatArchive, ChatImage, ChatLine, ChatScope, MAX_GROUP_GAP, SystemKind, group, starts_group,
};
pub use events::{ConnectionState, EventKind, SessionEvent};
pub use round_trip::{RoundTripMeter, SMOOTHING};
pub use session::{ConnectOptions, FileListing, ReceivedVoice, Session, SessionConfig};
pub use snapshot::{ChannelInfo, ClientInfo, ServerSnapshot, StreamInfo};
