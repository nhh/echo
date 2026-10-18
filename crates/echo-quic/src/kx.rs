//! Schluesseltausch mit X25519. Eine Gruppe reicht: beide Seiten sind echo.

use rand_core::OsRng;
use rustls::crypto::{ActiveKeyExchange, SharedSecret, SupportedKxGroup};
use rustls::{Error, NamedGroup, PeerMisbehaved};
use x25519_dalek::{EphemeralSecret, PublicKey};

pub(crate) static X25519: X25519Group = X25519Group;

#[derive(Debug)]
pub(crate) struct X25519Group;

impl SupportedKxGroup for X25519Group {
    fn start(&self) -> Result<Box<dyn ActiveKeyExchange>, Error> {
        let secret = EphemeralSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);
        Ok(Box::new(X25519Exchange { secret, public }))
    }

    fn name(&self) -> NamedGroup {
        NamedGroup::X25519
    }
}

struct X25519Exchange {
    secret: EphemeralSecret,
    public: PublicKey,
}

impl ActiveKeyExchange for X25519Exchange {
    fn complete(self: Box<Self>, peer_pub_key: &[u8]) -> Result<SharedSecret, Error> {
        let peer: [u8; 32] = peer_pub_key
            .try_into()
            .map_err(|_| PeerMisbehaved::InvalidKeyShare)?;
        let shared = self.secret.diffie_hellman(&PublicKey::from(peer));
        // Ein Punkt kleiner Ordnung ergaebe ein Geheimnis, das der Angreifer kennt
        // (RFC 8446, 7.4.2).
        if !shared.was_contributory() {
            return Err(PeerMisbehaved::InvalidKeyShare.into());
        }
        Ok(SharedSecret::from(&shared.as_bytes()[..]))
    }

    fn pub_key(&self) -> &[u8] {
        self.public.as_bytes()
    }

    fn group(&self) -> NamedGroup {
        NamedGroup::X25519
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_sides_agree() {
        let alice = X25519.start().unwrap();
        let bob = X25519.start().unwrap();
        let alice_public = alice.pub_key().to_vec();
        let bob_public = bob.pub_key().to_vec();

        let from_alice = alice.complete(&bob_public).unwrap();
        let from_bob = bob.complete(&alice_public).unwrap();
        assert_eq!(from_alice.secret_bytes(), from_bob.secret_bytes());
    }

    #[test]
    fn rejects_malformed_and_low_order_keys() {
        assert!(X25519.start().unwrap().complete(&[1; 31]).is_err());
        // Der Nullpunkt hat kleine Ordnung.
        assert!(X25519.start().unwrap().complete(&[0; 32]).is_err());
    }

    #[test]
    fn names_its_group() {
        assert_eq!(X25519.name(), NamedGroup::X25519);
        assert_eq!(X25519.start().unwrap().group(), NamedGroup::X25519);
    }
}
