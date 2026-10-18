//! Was die Session der Oberflaeche meldet. Die UI leert die Queue je Frame;
//! Ereignisse sind unveraenderlich und tragen alles, was zum Anzeigen noetig
//! ist — die UI muss nicht zurueckfragen.

use std::time::SystemTime;

use echo_protocol::ShapeDto;

use crate::chat::{ChatArchive, ChatLine};
use crate::session::ConnectOptions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionEvent {
    pub time: SystemTime,
    pub kind: EventKind,
}

impl SessionEvent {
    pub(crate) fn now(kind: EventKind) -> Self {
        Self {
            time: SystemTime::now(),
            kind,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EventKind {
    Connected {
        server_name: String,
    },
    Disconnected {
        reason: String,
        was_error: bool,
    },
    /// Der Server zeigt einen anderen Host-Schluessel als beim letzten Mal; die
    /// Verbindung ist abgelehnt. Die UI fragt wie SSH nach (echo-2) und
    /// verbindet mit `options` neu, wenn der Nutzer `offered` annimmt.
    HostKeyChanged {
        host: String,
        known: String,
        offered: String,
        options: Box<ConnectOptions>,
    },
    Chat(ChatLine),
    /// Aeltere Nachrichten, nachgeladen beim Hochscrollen (echo-182); leer
    /// heisst: es gibt nichts Aelteres.
    OlderHistory {
        archive: ChatArchive,
        lines: Vec<ChatLine>,
    },
    ClientJoined {
        client_id: u16,
        name: String,
        channel: String,
    },
    ClientLeft {
        client_id: u16,
        name: String,
        reason: String,
    },
    ClientMoved {
        client_id: u16,
        name: String,
        from: String,
        to: String,
        invoker: Option<String>,
        is_self: bool,
    },
    Poke {
        from_id: u16,
        from: String,
        text: String,
    },
    /// Moderation: so viele klopfen an (echo-39).
    ApprovalsChanged {
        pending: u32,
    },
    FilesChanged {
        channel: u64,
    },
    BoardsChanged {
        channel: u64,
    },
    BoardOpened {
        board_id: String,
        name: String,
        participants: Vec<u16>,
    },
    /// `own`: das Echo einer eigenen Aenderung.
    BoardEdited {
        board_id: String,
        by: u16,
        own: bool,
        changes: Vec<ShapeDto>,
    },
    BoardPresence {
        board_id: String,
        participants: Vec<u16>,
    },
    BoardPointer {
        board_id: String,
        by: u16,
        x: f32,
        y: f32,
        laser: bool,
    },
    Error(String),
    Info(String),
}
