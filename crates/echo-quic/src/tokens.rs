//! Die Schluessel, die quinn neben TLS braucht: HMAC fuer Stateless Resets
//! und HKDF plus AES-256-GCM fuer Adress-Tokens. Gleiche Verfahren wie
//! quinns eigene Umsetzung mit ring (`crypto/ring_like.rs`).

use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::{AeadInPlace, Aes256Gcm, KeyInit, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use quinn_proto::crypto::{AeadKey, CryptoError, HandshakeTokenKey, HmacKey};
use sha2::Sha256;

use crate::digest::new_hmac;

const RESET_KEY_LEN: usize = 64;
const MASTER_KEY_LEN: usize = 64;
const TOKEN_KEY_LEN: usize = 32;
const HMAC_LEN: usize = 32;

/// Signiert Stateless-Reset-Tokens.
pub(crate) struct ResetKey(Hmac<Sha256>);

impl ResetKey {
    pub(crate) fn random() -> Result<Self, getrandom::Error> {
        let mut key = [0u8; RESET_KEY_LEN];
        getrandom::getrandom(&mut key)?;
        Ok(Self(new_hmac(&key)))
    }
}

impl HmacKey for ResetKey {
    fn sign(&self, data: &[u8], signature_out: &mut [u8]) {
        let mut mac = self.0.clone();
        mac.update(data);
        signature_out.copy_from_slice(&mac.finalize().into_bytes());
    }

    fn signature_len(&self) -> usize {
        HMAC_LEN
    }

    fn verify(&self, data: &[u8], signature: &[u8]) -> Result<(), CryptoError> {
        let mut mac = self.0.clone();
        mac.update(data);
        mac.verify_slice(signature).map_err(|_| CryptoError)
    }
}

/// Leitet je Token einen eigenen AES-256-GCM-Schluessel ab.
pub(crate) struct TokenKey(Hkdf<Sha256>);

impl TokenKey {
    pub(crate) fn random() -> Result<Self, getrandom::Error> {
        let mut master = [0u8; MASTER_KEY_LEN];
        getrandom::getrandom(&mut master)?;
        Ok(Self(Hkdf::<Sha256>::new(None, &master)))
    }
}

impl HandshakeTokenKey for TokenKey {
    fn aead_from_hkdf(&self, random_bytes: &[u8]) -> Box<dyn AeadKey> {
        let mut key = [0u8; TOKEN_KEY_LEN];
        // 32 Byte liegen weit unter der Grenze von HKDF-SHA-256 (255 * 32).
        if self.0.expand(random_bytes, &mut key).is_err() {
            unreachable!("HKDF-SHA-256 liefert 32 Byte");
        }
        Box::new(TokenCipher(Aes256Gcm::new(GenericArray::from_slice(&key))))
    }
}

/// Jeder Schluessel verschluesselt genau ein Token; darum reicht die Null-Nonce.
struct TokenCipher(Aes256Gcm);

const ZERO_NONCE: [u8; 12] = [0; 12];

impl AeadKey for TokenCipher {
    fn seal(&self, data: &mut Vec<u8>, additional_data: &[u8]) -> Result<(), CryptoError> {
        self.0
            .encrypt_in_place(Nonce::from_slice(&ZERO_NONCE), additional_data, data)
            .map_err(|_| CryptoError)
    }

    fn open<'a>(
        &self,
        data: &'a mut [u8],
        additional_data: &[u8],
    ) -> Result<&'a mut [u8], CryptoError> {
        const TAG_LEN: usize = 16;
        let plain_len = data.len().checked_sub(TAG_LEN).ok_or(CryptoError)?;
        let (plain, tag) = data.split_at_mut(plain_len);
        self.0
            .decrypt_in_place_detached(
                Nonce::from_slice(&ZERO_NONCE),
                additional_data,
                plain,
                GenericArray::from_slice(tag),
            )
            .map_err(|_| CryptoError)?;
        Ok(plain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_signatures_verify() {
        let key = ResetKey::random().unwrap();
        let mut signature = [0u8; HMAC_LEN];
        key.sign(b"verbindung", &mut signature);
        assert!(key.verify(b"verbindung", &signature).is_ok());
        assert!(key.verify(b"andere", &signature).is_err());
        assert_eq!(key.signature_len(), HMAC_LEN);
    }

    #[test]
    fn tokens_round_trip_and_detect_tampering() {
        let key = TokenKey::random().unwrap();
        let cipher = key.aead_from_hkdf(b"zufall");
        let mut data = b"adresse".to_vec();
        cipher.seal(&mut data, b"aad").unwrap();

        let mut tampered = data.clone();
        tampered[0] ^= 1;
        assert!(cipher.open(&mut tampered, b"aad").is_err());
        assert!(cipher.open(&mut data.clone(), b"anderes").is_err());
        assert!(cipher.open(&mut [0; 3], b"aad").is_err());
        assert!(
            key.aead_from_hkdf(b"anderer zufall")
                .open(&mut data.clone(), b"aad")
                .is_err()
        );

        assert_eq!(cipher.open(&mut data, b"aad").unwrap(), b"adresse");
    }
}
