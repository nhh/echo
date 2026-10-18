//! Chatzeilen, wie das Chatfenster sie zeigt, und die Archive, aus denen sie
//! kommen (echo-182).

use std::time::{Duration, SystemTime};

use echo_protocol::{ArchiveRef, ChatScope as WireScope};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChatScope {
    Server,
    Channel,
    Private,
    /// Meldungen des Clients selbst (Verbindung, Fehler), nicht vom Server.
    System,
}

/// Worum es in einer Systemzeile geht; der Chat zeigt danach das Symbol
/// (echo-110). Anstupsen meint dich direkt und wird hervorgehoben.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemKind {
    Info,
    Join,
    Leave,
    Away,
    Poke,
    Error,
}

/// Welches Archiv auf dem Server: Serverchat, ein Kanal oder ein privates
/// Gespraech. `peer` ist bei privaten der Fingerabdruck des Gegenuebers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChatArchive {
    pub scope: ChatScope,
    pub channel: u64,
    pub peer: String,
}

impl ChatArchive {
    #[must_use]
    pub fn server() -> Self {
        Self {
            scope: ChatScope::Server,
            channel: 0,
            peer: String::new(),
        }
    }

    #[must_use]
    pub fn channel(channel: u64) -> Self {
        Self {
            scope: ChatScope::Channel,
            channel,
            peer: String::new(),
        }
    }

    #[must_use]
    pub fn private(peer: &str) -> Self {
        Self {
            scope: ChatScope::Private,
            channel: 0,
            peer: peer.to_owned(),
        }
    }

    pub(crate) fn from_wire(archive: &ArchiveRef) -> Self {
        match archive.scope {
            WireScope::Server => Self::server(),
            WireScope::Channel => Self::channel(archive.channel),
            WireScope::Private => Self::private(&archive.peer),
        }
    }

    /// `None` fuer Systemzeilen: die gibt es nur hier.
    pub(crate) fn to_wire(&self) -> Option<ArchiveRef> {
        let scope = match self.scope {
            ChatScope::Server => WireScope::Server,
            ChatScope::Channel => WireScope::Channel,
            ChatScope::Private => WireScope::Private,
            ChatScope::System => return None,
        };
        Some(ArchiveRef {
            scope,
            channel: self.channel,
            peer: self.peer.clone(),
        })
    }
}

/// Ein Bild im Chat (echo-166): Kennung auf dem Server und die Masse, die der
/// Absender nannte, damit der Verlauf Platz dafuer laesst.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatImage {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub size: u64,
}

/// Eine Zeile im Chatfenster. Mit `image` ist `text` leer; `seq` ist die
/// Nummer im Archiv des Servers, 0 fuer Systemzeilen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatLine {
    pub time: SystemTime,
    pub archive: ChatArchive,
    pub from: String,
    pub text: String,
    pub own: bool,
    /// Bei privaten Gespraechen der Name des Gegenuebers, fuer den Reiter.
    pub peer_name: String,
    pub kind: Option<SystemKind>,
    pub image: Option<ChatImage>,
    pub seq: i64,
}

impl ChatLine {
    /// Eine Meldung des Clients selbst.
    #[must_use]
    pub fn system(kind: SystemKind, text: &str) -> Self {
        Self {
            time: SystemTime::now(),
            archive: ChatArchive {
                scope: ChatScope::System,
                channel: 0,
                peer: String::new(),
            },
            from: String::new(),
            text: text.to_owned(),
            own: false,
            peer_name: String::new(),
            kind: Some(kind),
            image: None,
            seq: 0,
        }
    }
}

/// Laenger als das zwischen zwei Nachrichten, und der Name steht neu da.
pub const MAX_GROUP_GAP: Duration = Duration::from_mins(5);
const SECONDS_PER_DAY: u64 = 24 * 60 * 60;

/// Beginnt `line` eine neue Gruppe nach `previous` (echo-98)? Der Chat
/// zeigt Name und Zeit nur einmal je Gruppe.
#[must_use]
pub fn starts_group(previous: Option<&ChatLine>, line: &ChatLine) -> bool {
    let Some(previous) = previous else {
        return true;
    };
    let gap = line.time.duration_since(previous.time);
    line.archive.scope == ChatScope::System
        || previous.archive.scope == ChatScope::System
        || previous.from != line.from
        || previous.own != line.own
        || gap.is_err()
        || gap.is_ok_and(|gap| gap > MAX_GROUP_GAP)
        // Zwischen zwei Tagen steht ein Trenner (echo-109); danach neu.
        || day(previous.time) != day(line.time)
}

/// Die Zeilen in Gruppen, in ihrer Reihenfolge.
#[must_use]
pub fn group(lines: &[ChatLine]) -> Vec<Vec<&ChatLine>> {
    let mut groups: Vec<Vec<&ChatLine>> = Vec::new();
    let mut previous = None;
    for line in lines {
        match groups.last_mut() {
            Some(current) if !starts_group(previous, line) => current.push(line),
            _ => groups.push(vec![line]),
        }
        previous = Some(line);
    }
    groups
}

/// Der Tag in UTC; fuer den Trenner reicht das, die Zeitzone setzt die UI.
fn day(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / SECONDS_PER_DAY)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(from: &str, seconds: u64) -> ChatLine {
        ChatLine {
            time: SystemTime::UNIX_EPOCH + Duration::from_secs(seconds),
            archive: ChatArchive::server(),
            from: from.into(),
            text: "x".into(),
            own: false,
            peer_name: String::new(),
            kind: None,
            image: None,
            seq: 1,
        }
    }

    #[test]
    fn messages_group_by_author_time_and_day() {
        let lines = [
            line("Alice", 100),
            line("Alice", 160),
            line("Bob", 170),
            line("Bob", 170 + 6 * 60),
            line("Bob", SECONDS_PER_DAY + 10),
            line("Bob", SECONDS_PER_DAY + 5),
        ];
        let sizes: Vec<_> = group(&lines).iter().map(Vec::len).collect();
        assert_eq!(sizes, [2, 1, 1, 1, 1]);
        assert!(starts_group(
            Some(&lines[0]),
            &ChatLine::system(SystemKind::Info, "x")
        ));
    }

    #[test]
    fn archives_map_to_the_wire_and_back() {
        for archive in [
            ChatArchive::server(),
            ChatArchive::channel(3),
            ChatArchive::private("SHA256:p"),
        ] {
            assert_eq!(ChatArchive::from_wire(&archive.to_wire().unwrap()), archive);
        }
        assert!(
            ChatLine::system(SystemKind::Error, "x")
                .archive
                .to_wire()
                .is_none()
        );
    }
}
