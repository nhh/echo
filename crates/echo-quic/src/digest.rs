//! SHA-256 und HMAC-SHA-256 fuer rustls. Daraus baut rustls auch HKDF
//! (`HkdfUsingHmac`), das QUIC fuer alle Schluessel braucht.

use hmac::{Hmac, Mac};
use rustls::crypto::{hash, hmac as tls_hmac};
use sha2::{Digest, Sha256};

const SHA256_LEN: usize = 32;

pub(crate) static SHA256: Sha256Hash = Sha256Hash;
pub(crate) static HMAC_SHA256: HmacSha256 = HmacSha256;

pub(crate) struct Sha256Hash;

impl hash::Hash for Sha256Hash {
    fn start(&self) -> Box<dyn hash::Context> {
        Box::new(Sha256Context(Sha256::new()))
    }

    fn hash(&self, data: &[u8]) -> hash::Output {
        hash::Output::new(&Sha256::digest(data))
    }

    fn output_len(&self) -> usize {
        SHA256_LEN
    }

    fn algorithm(&self) -> hash::HashAlgorithm {
        hash::HashAlgorithm::SHA256
    }
}

struct Sha256Context(Sha256);

impl hash::Context for Sha256Context {
    fn fork_finish(&self) -> hash::Output {
        hash::Output::new(&self.0.clone().finalize())
    }

    fn fork(&self) -> Box<dyn hash::Context> {
        Box::new(Self(self.0.clone()))
    }

    fn finish(self: Box<Self>) -> hash::Output {
        hash::Output::new(&self.0.finalize())
    }

    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }
}

pub(crate) struct HmacSha256;

impl tls_hmac::Hmac for HmacSha256 {
    fn with_key(&self, key: &[u8]) -> Box<dyn tls_hmac::Key> {
        Box::new(HmacSha256Key(new_hmac(key)))
    }

    fn hash_output_len(&self) -> usize {
        SHA256_LEN
    }
}

struct HmacSha256Key(Hmac<Sha256>);

impl tls_hmac::Key for HmacSha256Key {
    fn sign_concat(&self, first: &[u8], middle: &[&[u8]], last: &[u8]) -> tls_hmac::Tag {
        let mut mac = self.0.clone();
        mac.update(first);
        for part in middle {
            mac.update(part);
        }
        mac.update(last);
        tls_hmac::Tag::new(&mac.finalize().into_bytes())
    }

    fn tag_len(&self) -> usize {
        SHA256_LEN
    }
}

/// HMAC nimmt Schluessel jeder Laenge; der Fehlerfall der API tritt nicht ein.
pub(crate) fn new_hmac(key: &[u8]) -> Hmac<Sha256> {
    match Hmac::<Sha256>::new_from_slice(key) {
        Ok(mac) => mac,
        Err(_) => unreachable!("HMAC nimmt Schluessel jeder Laenge"),
    }
}

#[cfg(test)]
mod tests {
    use rustls::crypto::hash::Hash;
    use rustls::crypto::hmac::Hmac as _;

    use super::*;

    /// RFC 4231, Testfall 2.
    #[test]
    fn hmac_matches_rfc_4231() {
        let key = HMAC_SHA256.with_key(b"Jefe");
        let tag = key.sign(&[b"what do ya want ", b"for nothing?"]);
        assert_eq!(
            hex(tag.as_ref()),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn incremental_hash_equals_one_shot() {
        let mut context = SHA256.start();
        context.update(b"ab");
        let forked = context.fork_finish();
        context.update(b"c");
        assert_eq!(forked.as_ref(), SHA256.hash(b"ab").as_ref());
        assert_eq!(context.finish().as_ref(), SHA256.hash(b"abc").as_ref());
        assert_eq!(
            hex(SHA256.hash(b"abc").as_ref()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().fold(String::new(), |mut hex, b| {
            let _ = std::fmt::Write::write_fmt(&mut hex, format_args!("{b:02x}"));
            hex
        })
    }
}
