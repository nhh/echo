//! Wer welche Tafel gerade ansieht und wer mitmacht (echo-183). Fluechtig:
//! gespeichert sind nur Tafeln und Formen, im Archiv ihres Kanals. Unter der
//! Sperre von `ServerState` benutzt; Clients stehen hier nur mit ihrer ID.

use std::collections::{BTreeSet, HashMap};

#[derive(Debug)]
struct Board {
    channel: u64,
    viewers: BTreeSet<u16>,
    participants: BTreeSet<u16>,
}

#[derive(Debug, Default)]
pub(crate) struct BoardRegistry {
    boards: HashMap<String, Board>,
}

impl BoardRegistry {
    /// Macht eine Tafel bekannt (beim Start und beim Anlegen).
    pub(crate) fn add(&mut self, id: &str, channel: u64) {
        self.boards.entry(id.to_owned()).or_insert(Board {
            channel,
            viewers: BTreeSet::new(),
            participants: BTreeSet::new(),
        });
    }

    pub(crate) fn remove(&mut self, id: &str) {
        self.boards.remove(id);
    }

    /// Die Tafeln eines Kanals, der verschwindet.
    pub(crate) fn remove_channel(&mut self, channel: u64) {
        self.boards.retain(|_, board| board.channel != channel);
    }

    pub(crate) fn channel_of(&self, id: &str) -> Option<u64> {
        self.boards.get(id).map(|b| b.channel)
    }

    pub(crate) fn view(&mut self, id: &str, client: u16) {
        if let Some(board) = self.boards.get_mut(id) {
            board.viewers.insert(client);
        }
    }

    pub(crate) fn is_viewing(&self, id: &str, client: u16) -> bool {
        self.boards
            .get(id)
            .is_some_and(|b| b.viewers.contains(&client))
    }

    pub(crate) fn is_participant(&self, id: &str, client: u16) -> bool {
        self.boards
            .get(id)
            .is_some_and(|b| b.participants.contains(&client))
    }

    /// Mitmachen oder nicht; `true`, wenn sich etwas geaendert hat.
    pub(crate) fn participate(&mut self, id: &str, client: u16, join: bool) -> bool {
        self.boards.get_mut(id).is_some_and(|board| {
            if join {
                board.participants.insert(client)
            } else {
                board.participants.remove(&client)
            }
        })
    }

    pub(crate) fn viewers(&self, id: &str) -> Vec<u16> {
        self.boards
            .get(id)
            .map(|b| b.viewers.iter().copied().collect())
            .unwrap_or_default()
    }

    pub(crate) fn participants(&self, id: &str) -> Vec<u16> {
        self.boards
            .get(id)
            .map(|b| b.participants.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Hoert auf, eine Tafel anzusehen; `true`, wenn er mitgemacht hatte.
    pub(crate) fn leave(&mut self, id: &str, client: u16) -> bool {
        self.boards.get_mut(id).is_some_and(|board| {
            board.viewers.remove(&client);
            board.participants.remove(&client)
        })
    }

    /// Die Tafeln, die `client` ansieht, ausser denen in `keep_channel`.
    pub(crate) fn viewed_by(&self, client: u16, keep_channel: Option<u64>) -> Vec<String> {
        self.boards
            .iter()
            .filter(|(_, b)| Some(b.channel) != keep_channel && b.viewers.contains(&client))
            .map(|(id, _)| id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewers_join_and_leave() {
        let mut boards = BoardRegistry::default();
        boards.add("b", 1);
        boards.view("b", 7);
        assert!(boards.is_viewing("b", 7));
        assert!(boards.participate("b", 7, true));
        assert!(!boards.participate("b", 7, true));
        assert_eq!(boards.participants("b"), [7]);
        assert_eq!(boards.viewed_by(7, None), ["b"]);
        assert!(boards.viewed_by(7, Some(1)).is_empty());
        assert!(boards.leave("b", 7));
        assert!(!boards.is_viewing("b", 7));
        assert!(boards.viewers("b").is_empty());
    }

    #[test]
    fn unknown_boards_are_harmless() {
        let mut boards = BoardRegistry::default();
        boards.view("x", 1);
        assert!(!boards.participate("x", 1, true));
        assert!(!boards.leave("x", 1));
        assert!(boards.participants("x").is_empty());
        assert_eq!(boards.channel_of("x"), None);
        boards.add("a", 2);
        boards.remove_channel(2);
        assert_eq!(boards.channel_of("a"), None);
        boards.add("c", 3);
        boards.remove("c");
        assert_eq!(boards.channel_of("c"), None);
    }
}
