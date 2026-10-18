//! Der Kanalbaum: anlegen, finden, entfernen, Reihenfolge, Persistenz. Kennt
//! weder Clients noch Verbindungen und verschickt nichts — wer wen
//! benachrichtigt, ordnet [`crate::state::ServerState`] an (echo-34).
//!
//! `order` ist die ID des Vorgaengers unter demselben Elternkanal, 0 fuer den
//! ersten. Die Clients loesen daraus die Reihenfolge auf; hier wird die Kette
//! nur gepflegt.

use std::collections::HashMap;

use echo_protocol::ChannelDto;

use crate::stores::StoredChannel;

const MAX_NAME_LENGTH: usize = 60;
const DEFAULT_NAME: &str = "Lobby";
/// Kein Limit fuer Clients im Kanal.
pub(crate) const UNLIMITED: i32 = -1;

/// Ein Kanal auf dem Server. Mehr als das DTO: das Passwort im Klartext und,
/// bei temporaeren Kanaelen, wer ihn angelegt hat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Channel {
    pub(crate) id: u64,
    pub(crate) parent: u64,
    pub(crate) order: u64,
    pub(crate) name: String,
    pub(crate) topic: Option<String>,
    pub(crate) password: Option<String>,
    pub(crate) is_default: bool,
    pub(crate) permanent: bool,
    pub(crate) max_clients: i32,
    pub(crate) needed_talk_power: i32,
    pub(crate) created_by: u16,
}

impl Channel {
    #[must_use]
    pub(crate) fn to_dto(&self) -> ChannelDto {
        ChannelDto {
            id: self.id,
            parent: self.parent,
            order: self.order,
            name: self.name.clone(),
            topic: self.topic.clone(),
            has_password: self.password.is_some(),
            is_default: self.is_default,
            permanent: self.permanent,
            max_clients: self.max_clients,
            needed_talk_power: self.needed_talk_power,
        }
    }

    fn to_stored(&self) -> StoredChannel {
        StoredChannel {
            id: self.id,
            parent: self.parent,
            order: self.order,
            name: self.name.clone(),
            topic: self.topic.clone(),
            password: self.password.clone(),
            is_default: self.is_default,
            max_clients: self.max_clients,
            needed_talk_power: self.needed_talk_power,
        }
    }
}

/// Was ein neuer Kanal braucht.
#[derive(Debug, Clone)]
pub(crate) struct NewChannel {
    pub(crate) name: String,
    pub(crate) parent: u64,
    pub(crate) permanent: bool,
    pub(crate) password: Option<String>,
    pub(crate) max_clients: i32,
    pub(crate) topic: Option<String>,
    pub(crate) created_by: u16,
}

#[derive(Debug)]
pub(crate) struct ChannelRegistry {
    channels: HashMap<u64, Channel>,
    next_id: u64,
}

impl ChannelRegistry {
    /// Aus der Tabelle; ein frischer Server bekommt eine Lobby, und es gibt
    /// immer genau einen Standardkanal. `changed` sagt, ob gespeichert
    /// werden muss.
    #[must_use]
    pub(crate) fn load(stored: Vec<StoredChannel>) -> (Self, bool) {
        let mut registry = Self {
            channels: HashMap::new(),
            next_id: 1,
        };
        for channel in stored {
            registry.next_id = registry.next_id.max(channel.id + 1);
            registry.channels.insert(
                channel.id,
                Channel {
                    id: channel.id,
                    parent: channel.parent,
                    order: channel.order,
                    name: channel.name,
                    topic: channel.topic,
                    password: channel.password,
                    is_default: channel.is_default,
                    permanent: true,
                    max_clients: channel.max_clients,
                    needed_talk_power: channel.needed_talk_power,
                    created_by: 0,
                },
            );
        }

        let mut changed = false;
        if registry.channels.is_empty() {
            let id = registry.take_id();
            registry.channels.insert(
                id,
                Channel {
                    id,
                    parent: 0,
                    order: 0,
                    name: DEFAULT_NAME.into(),
                    topic: None,
                    password: None,
                    is_default: true,
                    permanent: true,
                    max_clients: UNLIMITED,
                    needed_talk_power: 0,
                    created_by: 0,
                },
            );
            changed = true;
        }

        // Ein Server ohne Standardkanal haette keinen Ort fuer neue Clients.
        if !registry.channels.values().any(|c| c.is_default) {
            if let Some(first) = registry.channels.keys().min().copied()
                && let Some(channel) = registry.channels.get_mut(&first)
            {
                channel.is_default = true;
            }
            changed = true;
        }
        (registry, changed)
    }

    pub(crate) fn all(&self) -> impl Iterator<Item = &Channel> {
        self.channels.values()
    }

    /// Nach ID sortiert, fuer stabile Ausgaben.
    #[must_use]
    pub(crate) fn sorted(&self) -> Vec<&Channel> {
        let mut channels: Vec<_> = self.channels.values().collect();
        channels.sort_by_key(|c| c.id);
        channels
    }

    /// Der Kanal, in dem neue Clients landen; es gibt immer genau einen.
    #[must_use]
    pub(crate) fn default_channel(&self) -> &Channel {
        match self.channels.values().find(|c| c.is_default) {
            Some(channel) => channel,
            None => unreachable!("load und remove sorgen fuer einen Standardkanal"),
        }
    }

    #[must_use]
    pub(crate) fn get(&self, id: u64) -> Option<&Channel> {
        self.channels.get(&id)
    }

    pub(crate) fn get_mut(&mut self, id: u64) -> Option<&mut Channel> {
        self.channels.get_mut(&id)
    }

    #[must_use]
    pub(crate) fn exists(&self, id: u64) -> bool {
        self.channels.contains_key(&id)
    }

    #[must_use]
    pub(crate) fn children(&self, parent: u64) -> Vec<u64> {
        let mut children: Vec<_> = self
            .channels
            .values()
            .filter(|c| c.parent == parent)
            .map(|c| c.id)
            .collect();
        children.sort_unstable();
        children
    }

    /// Die Kanaele, deren Vorgaenger dieser ist — sie ruecken auf, wenn er geht.
    #[must_use]
    pub(crate) fn successors(&self, id: u64) -> Vec<u64> {
        let Some(channel) = self.channels.get(&id) else {
            return Vec::new();
        };
        self.channels
            .values()
            .filter(|c| c.parent == channel.parent && c.order == id)
            .map(|c| c.id)
            .collect()
    }

    #[must_use]
    pub(crate) fn is_valid_name(name: &str) -> bool {
        (1..=MAX_NAME_LENGTH).contains(&name.chars().count())
    }

    /// Haengt den Kanal hinten an seine Geschwister an.
    pub(crate) fn add(&mut self, new: NewChannel) -> &Channel {
        let id = self.take_id();
        let order = self.last_sibling(new.parent);
        let channel = Channel {
            id,
            parent: new.parent,
            order,
            name: new.name,
            topic: new.topic,
            password: new.password.filter(|p| !p.is_empty()),
            is_default: false,
            permanent: new.permanent,
            max_clients: new.max_clients,
            needed_talk_power: 0,
            created_by: new.created_by,
        };
        self.channels.entry(id).or_insert(channel)
    }

    pub(crate) fn remove(&mut self, id: u64) {
        self.channels.remove(&id);
    }

    /// Nur dauerhafte Kanaele stehen in der Tabelle; temporaere sterben mit
    /// dem letzten Nutzer.
    #[must_use]
    pub(crate) fn stored(&self) -> Vec<StoredChannel> {
        self.sorted()
            .into_iter()
            .filter(|c| c.permanent)
            .map(Channel::to_stored)
            .collect()
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Die ID des letzten Geschwisterkanals (Ende der Kette), 0 wenn keiner.
    fn last_sibling(&self, parent: u64) -> u64 {
        let siblings: Vec<_> = self
            .channels
            .values()
            .filter(|c| c.parent == parent)
            .collect();
        siblings
            .iter()
            .find(|c| !siblings.iter().any(|s| s.order == c.id))
            .map_or(0, |c| c.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new(name: &str, parent: u64) -> NewChannel {
        NewChannel {
            name: name.into(),
            parent,
            permanent: false,
            password: Some(String::new()),
            max_clients: UNLIMITED,
            topic: None,
            created_by: 7,
        }
    }

    #[test]
    fn a_fresh_server_gets_a_lobby() {
        let (registry, changed) = ChannelRegistry::load(Vec::new());
        assert!(changed);
        assert_eq!(registry.default_channel().name, "Lobby");
        assert_eq!(registry.stored().len(), 1);
    }

    #[test]
    fn siblings_chain_by_their_predecessor() {
        let (mut registry, _) = ChannelRegistry::load(Vec::new());
        let a = registry.add(new("A", 0)).id;
        let b = registry.add(new("B", 0)).id;
        let child = registry.add(new("Kind", a)).id;
        assert_eq!(registry.get(a).unwrap().order, 1);
        assert_eq!(registry.get(b).unwrap().order, a);
        assert_eq!(registry.get(child).unwrap().order, 0);
        assert_eq!(registry.successors(a), [b]);
        assert_eq!(registry.children(a), [child]);
        assert!(registry.get(a).unwrap().password.is_none());
        assert!(registry.stored().iter().all(|c| c.id == 1));
        registry.remove(child);
        assert!(!registry.exists(child));
        assert!(registry.get_mut(a).is_some());
        assert_eq!(registry.all().count(), 3);
    }

    #[test]
    fn loading_without_default_picks_the_first() {
        let stored = |id| StoredChannel {
            id,
            parent: 0,
            order: 0,
            name: format!("K{id}"),
            topic: None,
            password: None,
            is_default: false,
            max_clients: UNLIMITED,
            needed_talk_power: 0,
        };
        let (mut registry, changed) = ChannelRegistry::load(vec![stored(4), stored(2)]);
        assert!(changed);
        assert_eq!(registry.default_channel().id, 2);
        assert_eq!(registry.add(new("Neu", 0)).id, 5);
    }

    #[test]
    fn names_have_a_length() {
        assert!(ChannelRegistry::is_valid_name("Lobby"));
        assert!(!ChannelRegistry::is_valid_name(""));
        assert!(!ChannelRegistry::is_valid_name(&"x".repeat(61)));
        assert!(ChannelRegistry::is_valid_name(&"ä".repeat(60)));
    }
}
