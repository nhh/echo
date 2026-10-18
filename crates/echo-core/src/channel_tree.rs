//! Ordnet Kanaele, wie der Server sie meint: `order` eines Kanals ist die ID
//! des Vorgaengers unter demselben Elternkanal (0 = erster). Daraus wird je
//! Elternkanal eine verkettete Liste aufgeloest. Kanaele, deren Kette
//! abreisst (Server im Umbau), haengen alphabetisch hinten an — nie verloren.

use std::collections::{HashMap, HashSet};

use crate::snapshot::{ChannelInfo, ClientInfo, ServerSnapshot};

/// Wie tief ein Pfad hoechstens verfolgt wird; schuetzt vor Kreisen.
const MAX_PATH_DEPTH: usize = 64;

#[must_use]
pub fn children_of(channels: &[ChannelInfo], parent: u64) -> Vec<&ChannelInfo> {
    let siblings: Vec<_> = channels.iter().filter(|c| c.parent == parent).collect();
    let mut by_order: HashMap<u64, &ChannelInfo> = HashMap::new();
    for channel in &siblings {
        // Bei doppeltem `order` (Server-Inkonsistenz) gewinnt die kleinere ID.
        by_order
            .entry(channel.order)
            .and_modify(|existing| {
                if channel.id < existing.id {
                    *existing = channel;
                }
            })
            .or_insert(channel);
    }

    let mut ordered = Vec::with_capacity(siblings.len());
    let mut seen = HashSet::new();
    let mut previous = 0;
    while let Some(next) = by_order.get(&previous) {
        if !seen.insert(next.id) {
            break;
        }
        ordered.push(*next);
        previous = next.id;
    }

    let mut orphans: Vec<_> = siblings
        .into_iter()
        .filter(|c| !seen.contains(&c.id))
        .collect();
    orphans.sort_by_key(|c| c.name.to_lowercase());
    ordered.extend(orphans);
    ordered
}

/// Clients eines Kanals: nach Talk Power, dann Name — wie im Originalclient.
#[must_use]
pub fn clients_in(clients: &[ClientInfo], channel: u64) -> Vec<&ClientInfo> {
    let mut members: Vec<_> = clients.iter().filter(|c| c.channel == channel).collect();
    members.sort_by(|a, b| {
        b.talk_power
            .cmp(&a.talk_power)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    members
}

/// Der Pfad eines Kanals von der Wurzel: fuer Titelzeilen und Tooltips.
#[must_use]
pub fn path_to(snapshot: &ServerSnapshot, channel: u64) -> Vec<&ChannelInfo> {
    let mut path = Vec::new();
    let mut current = snapshot.channel(channel);
    while let Some(c) = current {
        if path.len() >= MAX_PATH_DEPTH {
            break;
        }
        path.push(c);
        current = if c.parent == 0 {
            None
        } else {
            snapshot.channel(c.parent)
        };
    }
    path.reverse();
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use echo_protocol::Role;

    fn channel(id: u64, parent: u64, order: u64, name: &str) -> ChannelInfo {
        ChannelInfo {
            id,
            parent,
            order,
            name: name.into(),
            topic: None,
            has_password: false,
            is_default: false,
            max_clients: -1,
            needed_talk_power: 0,
        }
    }

    fn ids(channels: &[&ChannelInfo]) -> Vec<u64> {
        channels.iter().map(|c| c.id).collect()
    }

    #[test]
    fn follows_the_chain_and_keeps_orphans() {
        let channels = [
            channel(3, 0, 2, "Drei"),
            channel(1, 0, 0, "Eins"),
            channel(2, 0, 1, "Zwei"),
            channel(9, 0, 42, "b-Waise"),
            channel(8, 0, 43, "A-Waise"),
            channel(5, 1, 0, "Kind"),
        ];
        assert_eq!(ids(&children_of(&channels, 0)), [1, 2, 3, 8, 9]);
        assert_eq!(ids(&children_of(&channels, 1)), [5]);
        assert!(children_of(&channels, 7).is_empty());
    }

    #[test]
    fn duplicate_orders_and_cycles_lose_nothing() {
        let channels = [
            channel(4, 0, 0, "Vier"),
            channel(2, 0, 0, "Zwei"),
            channel(6, 0, 6, "Kreis"),
        ];
        // Waisen nach Namen: "Kreis" vor "Vier".
        assert_eq!(ids(&children_of(&channels, 0)), [2, 6, 4]);
    }

    #[test]
    fn clients_sort_by_talk_power_then_name() {
        let client = |id, name: &str, talk_power| ClientInfo {
            id,
            channel: 1,
            name: name.into(),
            fingerprint: String::new(),
            input_muted: false,
            output_muted: false,
            away_message: None,
            talk_power,
            role: Role::Member,
            is_self: false,
        };
        let clients = [
            client(1, "bob", 0),
            client(2, "Alice", 0),
            client(3, "Zed", 100),
        ];
        let names: Vec<_> = clients_in(&clients, 1)
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(names, ["Zed", "Alice", "bob"]);
    }

    #[test]
    fn paths_run_from_the_root() {
        let snapshot = ServerSnapshot {
            channels: vec![
                channel(1, 0, 0, "Oben"),
                channel(2, 1, 0, "Mitte"),
                channel(3, 2, 0, "Unten"),
            ],
            ..ServerSnapshot::default()
        };
        assert_eq!(ids(&path_to(&snapshot, 3)), [1, 2, 3]);
        assert!(path_to(&snapshot, 9).is_empty());
    }
}
