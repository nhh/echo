//! Die verbundenen Clients: IDs vergeben, Namen eindeutig halten, wer in
//! welchem Kanal ist. Aendernde Aufrufe laufen unter der Sperre von
//! `ServerState`; die Zuordnung Kanal -> Mitglieder liegt zusaetzlich als
//! unveraenderlicher Schnappschuss vor, den der Sprachweg fuer jedes Paket
//! ohne Sperre liest (echo-34).

use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;

use crate::client::Client;

const MAX_NAME_LENGTH: usize = 30;
const FALLBACK_NAME: &str = "Gast";

/// Kanal -> Mitglieder.
pub(crate) type Members = HashMap<u64, Vec<Arc<Client>>>;

#[derive(Debug)]
pub(crate) struct ClientRegistry {
    clients: HashMap<u16, Arc<Client>>,
    members: Arc<ArcSwap<Members>>,
    next_id: u16,
}

impl ClientRegistry {
    #[must_use]
    pub(crate) fn new(members: Arc<ArcSwap<Members>>) -> Self {
        Self {
            clients: HashMap::new(),
            members,
            next_id: 1,
        }
    }

    #[must_use]
    pub(crate) fn count(&self) -> usize {
        self.clients.len()
    }

    /// Nach ID sortiert, damit Ausgaben stabil sind.
    #[must_use]
    pub(crate) fn all(&self) -> Vec<Arc<Client>> {
        let mut clients: Vec<_> = self.clients.values().cloned().collect();
        clients.sort_by_key(|c| c.id());
        clients
    }

    #[must_use]
    pub(crate) fn get(&self, id: u16) -> Option<Arc<Client>> {
        self.clients.get(&id).cloned()
    }

    #[must_use]
    pub(crate) fn in_channel(&self, channel: u64) -> Vec<Arc<Client>> {
        self.all()
            .into_iter()
            .filter(|c| c.channel() == channel)
            .collect()
    }

    #[must_use]
    pub(crate) fn with_fingerprint(&self, fingerprint: &str) -> Vec<Arc<Client>> {
        self.all()
            .into_iter()
            .filter(|c| c.fingerprint() == fingerprint)
            .collect()
    }

    pub(crate) fn add(&mut self, client: Arc<Client>) {
        self.clients.insert(client.id(), client);
        self.rebuild();
    }

    pub(crate) fn remove(&mut self, id: u16) -> bool {
        let removed = self.clients.remove(&id).is_some();
        if removed {
            self.rebuild();
        }
        removed
    }

    pub(crate) fn move_to(&mut self, client: &Client, channel: u64) {
        client.set_channel(channel);
        self.rebuild();
    }

    /// Die naechste freie ID; 0 bleibt frei, es steht fuer "niemand".
    pub(crate) fn next_id(&mut self) -> Option<u16> {
        for _ in 0..u16::MAX {
            let id = self.next_id;
            self.next_id = self.next_id.checked_add(1).unwrap_or(1);
            if id != 0 && !self.clients.contains_key(&id) {
                return Some(id);
            }
        }
        None
    }

    /// Haengt Ziffern an, bis der Name frei ist — zwei Clients mit demselben
    /// Namen waeren im Baum nicht auseinanderzuhalten. Der Client selbst
    /// zaehlt nicht mit, wenn er sich umbenennt.
    #[must_use]
    pub(crate) fn unique_name(&self, wanted: &str, except: Option<u16>) -> String {
        let trimmed = wanted.trim();
        let name: String = if trimmed.is_empty() {
            FALLBACK_NAME.into()
        } else {
            trimmed.chars().take(MAX_NAME_LENGTH).collect()
        };
        let taken = |candidate: &str| {
            self.clients.values().any(|c| {
                Some(c.id()) != except && c.name().to_lowercase() == candidate.to_lowercase()
            })
        };
        let mut candidate = name.clone();
        let mut n = 1;
        while taken(&candidate) {
            candidate = format!("{name}{n}");
            n += 1;
        }
        candidate
    }

    fn rebuild(&self) {
        let mut members = Members::new();
        for client in self.all() {
            members.entry(client.channel()).or_default().push(client);
        }
        self.members.store(Arc::new(members));
    }
}
