//! Angefragte, noch nicht ausgefuehrte Uebertragungen. Der Steuerkanal
//! vergibt das Token, die Datenverbindung loest es ein — einmal und nur kurz
//! gueltig, damit ein abgefangenes Token nichts mehr wert ist (echo-20).
//! Klein, kurzlebig und unter der Sperre von `ServerState` benutzt.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use echo_protocol::ChatScope;

const LIFETIME: Duration = Duration::from_secs(60);

/// Ein Chatbild auf dem Weg (echo-166): beim Hochladen mit seinem Ziel, beim
/// Herunterladen mit dem Archiv, aus dem es kommt; `peer` ist dort das
/// Gegenueber eines privaten Gespraechs (echo-182).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingImage {
    pub(crate) id: String,
    pub(crate) scope: ChatScope,
    pub(crate) channel: u64,
    pub(crate) target: u16,
    pub(crate) from: u16,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) peer: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Direction {
    Upload,
    /// Ab `offset` (echo-178); `streaming` heisst, Kino, Radio oder
    /// Bildbetrachter lesen direkt, und Auflegen ist kein Abbruch.
    Download {
        offset: u64,
        streaming: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingTransfer {
    pub(crate) token: String,
    pub(crate) direction: Direction,
    pub(crate) channel: u64,
    pub(crate) folder: String,
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) file_id: Option<String>,
    pub(crate) by: String,
    pub(crate) by_name: String,
    pub(crate) image: Option<PendingImage>,
    expires: Instant,
}

/// Was eine Uebertragung braucht, ohne Token und Ablauf.
#[derive(Debug, Clone)]
pub(crate) struct TransferRequest {
    pub(crate) direction: Direction,
    pub(crate) channel: u64,
    pub(crate) folder: String,
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) file_id: Option<String>,
    pub(crate) by: String,
    pub(crate) by_name: String,
    pub(crate) image: Option<PendingImage>,
}

#[derive(Debug, Default)]
pub(crate) struct TransferRegistry {
    pending: HashMap<String, PendingTransfer>,
}

impl TransferRegistry {
    /// Stellt eine Eintrittskarte aus; `token` kommt vom Aufrufer, damit er
    /// den Zufall bestimmt.
    pub(crate) fn issue(&mut self, token: String, request: TransferRequest) -> PendingTransfer {
        self.expire();
        let transfer = PendingTransfer {
            token: token.clone(),
            direction: request.direction,
            channel: request.channel,
            folder: request.folder,
            name: request.name,
            size: request.size,
            file_id: request.file_id,
            by: request.by,
            by_name: request.by_name,
            image: request.image,
            expires: Instant::now() + LIFETIME,
        };
        self.pending.insert(token, transfer.clone());
        transfer
    }

    /// Loest ein Token ein; danach ist es verbraucht.
    pub(crate) fn redeem(&mut self, token: &str) -> Option<PendingTransfer> {
        self.expire();
        self.pending.remove(token)
    }

    fn expire(&mut self) {
        let now = Instant::now();
        self.pending.retain(|_, transfer| transfer.expires > now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> TransferRequest {
        TransferRequest {
            direction: Direction::Upload,
            channel: 1,
            folder: String::new(),
            name: "x".into(),
            size: 3,
            file_id: None,
            by: "SHA256:a".into(),
            by_name: "Alice".into(),
            image: None,
        }
    }

    #[test]
    fn tokens_are_redeemed_once() {
        let mut registry = TransferRegistry::default();
        registry.issue("t".into(), request());
        assert_eq!(registry.redeem("t").unwrap().name, "x");
        assert!(registry.redeem("t").is_none());
        assert!(registry.redeem("fremd").is_none());
    }

    #[test]
    fn expired_tokens_are_gone() {
        let mut registry = TransferRegistry::default();
        let mut transfer = registry.issue("t".into(), request());
        transfer.expires = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
        registry.pending.insert("t".into(), transfer);
        assert!(registry.redeem("t").is_none());
    }
}
