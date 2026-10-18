//! Was der Server schickt: Antworten an ihre Anfrage, Ereignisse in Modell
//! und Queue, Nachrichten in den Spiegel.

use std::time::{Duration, SystemTime};

use echo_protocol::{ChatMessageDto, Envelope, LeaveReason, Message};
use echo_storage::{ArchivedImage, ArchivedMessage};

use super::Session;
use crate::chat::{ChatArchive, ChatImage, ChatLine, ChatScope};
use crate::events::EventKind;
use crate::mirror::Entry;

/// So viele Seiten holt ein Nachholen hoechstens; wer laenger weg war,
/// bekommt nur die neueste Seite, der Rest kommt beim Hochscrollen.
const MAX_SYNC_PAGES: u32 = 5;

/// So viele Nachrichten je Archiv zeigt der Verlauf beim Verbinden.
pub(crate) const SHOWN_ON_CONNECT: u32 = 200;

impl Session {
    pub(super) fn handle(&self, envelope: Envelope) {
        let Envelope {
            request_id,
            message,
        } = envelope;
        // Die Antwort auf OpenBoard vor den Formen, die gleich danach als
        // Ereignisse kommen: die Oberflaeche muss das Oeffnen zuerst sehen
        // (echo-183).
        if let Message::BoardSnapshot {
            board_id,
            name,
            participants,
        } = &message
        {
            self.publish(EventKind::BoardOpened {
                board_id: board_id.clone(),
                name: name.clone(),
                participants: participants.clone(),
            });
        }
        // Alles mit einer Anfrage-Id ist eine Antwort, egal welcher Typ;
        // Ereignisse tragen nie eine (echo-48).
        if request_id > 0 {
            self.resolve(request_id, message);
            return;
        }
        match message {
            Message::ChannelAdded { channel } | Message::ChannelChanged { channel } => {
                self.mutate(|m| {
                    m.channels.insert(channel.id, channel);
                });
            }
            Message::ChannelRemoved { channel_id } => self.mutate(|m| {
                m.channels.remove(&channel_id);
            }),
            Message::ClientChanged { client } => self.mutate(|m| {
                m.clients.insert(client.id, client);
            }),
            Message::StreamStarted { stream } => self.mutate(|m| {
                m.streams.insert(stream.id, stream);
            }),
            Message::StreamEnded { stream_id } => self.mutate(|m| {
                m.streams.remove(&stream_id);
            }),
            Message::ClientJoined { client } => self.client_joined(client),
            Message::ClientLeft {
                client_id,
                reason,
                detail,
            } => self.client_left(client_id, reason, detail.as_deref()),
            Message::ClientMoved {
                client_id,
                channel_id,
                invoker_id,
            } => {
                self.client_moved(client_id, channel_id, invoker_id);
            }
            Message::ChatEvent {
                archive, message, ..
            } => {
                let archive = ChatArchive::from_wire(&archive);
                self.take(&archive, vec![entry(&message)]);
            }
            Message::DmConversations { conversations } => self.conversations(conversations),
            Message::Poked {
                from_id,
                from_name,
                text,
            } => {
                self.publish(EventKind::Poke {
                    from_id,
                    from: from_name,
                    text,
                });
            }
            Message::ApprovalsChanged { pending } => {
                self.publish(EventKind::ApprovalsChanged { pending });
            }
            Message::FilesChanged { channel } => self.publish(EventKind::FilesChanged { channel }),
            board @ (Message::BoardsChanged { .. }
            | Message::BoardEdited { .. }
            | Message::BoardPresence { .. }
            | Message::BoardPointerMoved { .. }) => self.board_event(board),
            Message::Kicked {
                from_server,
                reason,
                invoker_name,
                ..
            } => {
                self.kicked(from_server, &reason, invoker_name.as_deref());
            }
            // Alles andere gehoert zu Medien, die dieser Kern noch nicht
            // kennt, oder ist eine Antwort ohne Anfrage.
            _ => {}
        }
    }

    /// Ereignisse der Tafeln (echo-183).
    fn board_event(&self, message: Message) {
        match message {
            Message::BoardsChanged { channel } => {
                self.publish(EventKind::BoardsChanged { channel });
            }
            Message::BoardEdited {
                board_id,
                by,
                changes,
            } => {
                let own = by != 0 && by == self.snapshot().own_client_id;
                self.publish(EventKind::BoardEdited {
                    board_id,
                    by,
                    own,
                    changes,
                });
            }
            Message::BoardPresence {
                board_id,
                participants,
            } => {
                self.publish(EventKind::BoardPresence {
                    board_id,
                    participants,
                });
            }
            Message::BoardPointerMoved {
                board_id,
                by,
                x,
                y,
                laser,
            } => {
                self.publish(EventKind::BoardPointer {
                    board_id,
                    by,
                    x,
                    y,
                    laser,
                });
            }
            _ => {}
        }
    }

    fn resolve(&self, request_id: u32, message: Message) {
        let Some(live) = self.live() else {
            return;
        };
        let waiting = live.pending.lock().remove(&request_id);
        if let Some(waiting) = waiting {
            // Der Wartende kann schon aufgegeben haben; dann ist die Antwort egal.
            let _ = waiting.send(message);
        }
    }

    fn client_joined(&self, client: echo_protocol::ClientDto) {
        let (id, name, channel) = (client.id, client.name.clone(), client.channel);
        self.mutate(|m| {
            m.clients.insert(client.id, client);
        });
        let snapshot = self.snapshot();
        if id != snapshot.own_client_id {
            self.publish(EventKind::ClientJoined {
                client_id: id,
                name,
                channel: snapshot.channel_name(channel),
            });
        }
    }

    fn client_left(&self, client_id: u16, reason: LeaveReason, detail: Option<&str>) {
        let name = self.client_name(client_id);
        self.mutate(|m| {
            m.clients.remove(&client_id);
        });
        let detail = detail.unwrap_or_default();
        let reason = match reason {
            LeaveReason::Kicked => format!("wurde vom Server geworfen ({detail})"),
            LeaveReason::Banned => format!("wurde gebannt ({detail})"),
            LeaveReason::Timeout => "hat die Verbindung verloren".to_owned(),
            LeaveReason::Left | LeaveReason::ServerStopped => "hat den Server verlassen".to_owned(),
        };
        self.publish(EventKind::ClientLeft {
            client_id,
            name,
            reason,
        });
    }

    fn client_moved(&self, client_id: u16, channel_id: u64, invoker_id: u16) {
        let before = self.snapshot();
        let from = before
            .client(client_id)
            .map(|c| before.channel_name(c.channel))
            .unwrap_or_default();
        self.mutate(|m| {
            if let Some(client) = m.clients.get_mut(&client_id) {
                client.channel = channel_id;
            }
        });
        let invoker =
            (invoker_id != 0 && invoker_id != client_id).then(|| self.client_name(invoker_id));
        let is_self = client_id == before.own_client_id;
        self.publish(EventKind::ClientMoved {
            client_id,
            name: self.client_name(client_id),
            from,
            to: before.channel_name(channel_id),
            invoker,
            is_self,
        });
        if is_self {
            let session = self.clone();
            tokio::spawn(async move { session.sync(ChatArchive::channel(channel_id)).await });
        }
    }

    fn conversations(&self, conversations: Vec<echo_protocol::DmSummary>) {
        let mirror = self.mirror();
        for conversation in conversations {
            let archive = ChatArchive::private(&conversation.peer);
            self.shared
                .peer_names
                .lock()
                .insert(conversation.peer, conversation.peer_name);
            let known = mirror.as_ref().map_or(0, |m| m.last_seq(&archive));
            if conversation.last_seq > known {
                let session = self.clone();
                tokio::spawn(async move { session.sync(archive).await });
            }
        }
    }

    fn kicked(&self, from_server: bool, reason: &str, invoker: Option<&str>) {
        let invoker = invoker.unwrap_or("Server");
        if from_server {
            if let Some(live) = self.live() {
                *live.kicked.lock() =
                    Some(format!("{invoker} hat dich vom Server geworfen: {reason}"));
            }
        } else {
            self.publish(EventKind::Info(format!(
                "{invoker} hat dich aus dem Kanal geworfen: {reason}"
            )));
        }
    }

    fn client_name(&self, id: u16) -> String {
        self.snapshot()
            .client(id)
            .map_or_else(|| format!("#{id}"), |c| c.name.clone())
    }

    // --- Verlauf (echo-182) ---------------------------------------------------

    /// Holt nach, was seit der hoechsten Nummer im Spiegel dazukam. Ohne
    /// Spiegel oder nach zu langer Pause nur die neueste Seite.
    pub(super) async fn sync(&self, archive: ChatArchive) {
        let Some(wire) = archive.to_wire() else {
            return;
        };
        let mut after = self.mirror().map_or(0, |m| m.last_seq(&archive));
        let mut pages = 0;
        while after > 0 && pages < MAX_SYNC_PAGES {
            pages += 1;
            let Ok(Message::ChatHistory { messages, more, .. }) = self
                .ask(Message::SyncChat {
                    archive: wire.clone(),
                    after_seq: after,
                })
                .await
            else {
                return;
            };
            let Some(last) = messages.last().map(|m| m.seq) else {
                return;
            };
            self.take(&archive, messages.iter().map(entry).collect());
            if !more {
                return;
            }
            after = last;
        }
        if let Ok(Message::ChatHistory { messages, .. }) = self
            .ask(Message::LoadHistory {
                archive: wire,
                before_seq: i64::MAX,
            })
            .await
        {
            self.take(&archive, messages.iter().map(entry).collect());
        }
    }

    /// Neue Nachrichten: in den Spiegel und als Zeile in die Queue.
    fn take(&self, archive: &ChatArchive, entries: Vec<Entry>) {
        if let Some(mirror) = self.mirror() {
            // Der Spiegel ist nur ein Cache; ohne ihn laeuft der Chat weiter.
            let _ = mirror.store(archive, &entries);
        }
        for entry in entries {
            self.remember_peer(archive, &entry.message);
            self.publish(EventKind::Chat(self.line(archive, &entry)));
        }
    }

    /// Der Verlauf aus dem Spiegel, wie er beim Verbinden da ist:
    /// Serverchat, der eigene Kanal und die privaten Gespraeche, nach Zeit.
    /// Was seither neu ist, kommt danach als `Chat`-Ereignis.
    #[must_use]
    pub fn history(&self) -> Vec<ChatLine> {
        let Some(mirror) = self.mirror() else {
            return Vec::new();
        };
        let mut archives = vec![ChatArchive::server()];
        if let Some(own) = self.snapshot().own_channel() {
            archives.push(ChatArchive::channel(own.id));
        }
        archives.extend(mirror.conversations());
        let mut lines: Vec<_> = archives
            .iter()
            .flat_map(|archive| {
                mirror
                    .before(archive, i64::MAX, SHOWN_ON_CONNECT)
                    .into_iter()
                    .map(|entry| self.line(archive, &entry))
                    .collect::<Vec<_>>()
            })
            .collect();
        lines.sort_by_key(|l| l.time);
        lines
    }

    /// Die Seite vor `before`: aus dem Spiegel, wenn er sie lueckenlos hat,
    /// sonst vom Server. Kommt als `OlderHistory`, auch leer — dann gibt es
    /// nichts Aelteres.
    pub async fn load_older(&self, archive: ChatArchive, before: i64) {
        let mirror = self.mirror();
        let mut entries = mirror
            .as_ref()
            .map(|m| m.before(&archive, before, SHOWN_ON_CONNECT))
            .unwrap_or_default();
        if !is_contiguous(&entries, before)
            && let Some(wire) = archive.to_wire()
        {
            entries = match self
                .ask(Message::LoadHistory {
                    archive: wire,
                    before_seq: before,
                })
                .await
            {
                Ok(Message::ChatHistory { messages, .. }) => messages.iter().map(entry).collect(),
                _ => Vec::new(),
            };
            if let Some(mirror) = mirror {
                let _ = mirror.store(&archive, &entries);
            }
        }
        let lines = entries.iter().map(|e| self.line(&archive, e)).collect();
        self.publish(EventKind::OlderHistory { archive, lines });
    }

    /// Der Name des Gegenuebers eines privaten Gespraechs, fuer den Reiter.
    fn remember_peer(&self, archive: &ChatArchive, message: &ArchivedMessage) {
        if archive.scope == ChatScope::Private && message.author == archive.peer {
            self.shared
                .peer_names
                .lock()
                .insert(archive.peer.clone(), message.author_name.clone());
        }
    }

    fn peer_name(&self, peer: &str) -> String {
        if let Some(name) = self.shared.peer_names.lock().get(peer) {
            return name.clone();
        }
        self.snapshot()
            .clients
            .iter()
            .find(|c| c.fingerprint == peer)
            .map_or_else(|| "Unbekannt".to_owned(), |c| c.name.clone())
    }

    fn line(&self, archive: &ChatArchive, entry: &Entry) -> ChatLine {
        let message = &entry.message;
        let own = message.author == self.own_fingerprint();
        let peer_name = match (archive.scope, own) {
            (ChatScope::Private, true) => self.peer_name(&archive.peer),
            (ChatScope::Private, false) => message.author_name.clone(),
            _ => String::new(),
        };
        ChatLine {
            time: unix_millis(message.time),
            archive: archive.clone(),
            from: message.author_name.clone(),
            text: message.text.clone(),
            own,
            peer_name,
            kind: None,
            image: entry.image.as_ref().map(|i| ChatImage {
                id: i.id.clone(),
                width: u32::try_from(i.width).unwrap_or(0),
                height: u32::try_from(i.height).unwrap_or(0),
                size: i.size,
            }),
            seq: message.seq,
        }
    }
}

/// Hat der Spiegel die Seite vor `before` ohne Luecke?
fn is_contiguous(entries: &[Entry], before: i64) -> bool {
    let count = i64::try_from(entries.len()).unwrap_or(i64::MAX);
    match (entries.first(), entries.last()) {
        (Some(first), Some(last)) => {
            last.message.seq == before - 1 && first.message.seq == before - count
        }
        _ => false,
    }
}

fn entry(message: &ChatMessageDto) -> Entry {
    Entry {
        message: ArchivedMessage {
            seq: message.seq,
            time: message.time,
            author: message.author.clone(),
            author_name: message.author_name.clone(),
            text: message.text.clone(),
            image: message.image.as_ref().map(|i| i.id.clone()),
        },
        image: message.image.as_ref().map(|i| ArchivedImage {
            id: i.id.clone(),
            width: i64::from(i.width),
            height: i64::from(i.height),
            size: i.size,
        }),
    }
}

fn unix_millis(millis: i64) -> SystemTime {
    let offset = Duration::from_millis(millis.unsigned_abs());
    if millis >= 0 {
        SystemTime::UNIX_EPOCH + offset
    } else {
        SystemTime::UNIX_EPOCH - offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seq: i64) -> Entry {
        entry(&ChatMessageDto {
            seq,
            time: -1000,
            author: "a".into(),
            author_name: "A".into(),
            text: String::new(),
            image: None,
        })
    }

    #[test]
    fn a_page_from_the_mirror_needs_no_gap() {
        assert!(is_contiguous(&[at(3), at(4)], 5));
        assert!(!is_contiguous(&[at(2), at(4)], 5));
        assert!(!is_contiguous(&[at(3)], 5));
        assert!(!is_contiguous(&[], 5));
        assert!(unix_millis(-1000) < SystemTime::UNIX_EPOCH);
    }
}
