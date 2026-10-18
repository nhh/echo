//! ECDSA P-256 mit SHA-256: der Server signiert den Handschlag mit dem
//! Schluessel seines Zertifikats, der Client prueft die Signatur.

use std::fmt;
use std::sync::Arc;

use p256::ecdsa::signature::{Signer as _, Verifier as _};
use p256::ecdsa::{Signature, SigningKey as EcdsaSigningKey, VerifyingKey};
use p256::pkcs8::DecodePrivateKey;
use rustls::crypto::{KeyProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{
    AlgorithmIdentifier, InvalidSignature, PrivateKeyDer, SignatureVerificationAlgorithm, alg_id,
};
use rustls::sign::{Signer, SigningKey};
use rustls::{Error, SignatureAlgorithm, SignatureScheme};

pub(crate) static KEY_LOADER: KeyLoader = KeyLoader;

static ECDSA_P256_SHA256: EcdsaP256Sha256 = EcdsaP256Sha256;

pub(crate) static SUPPORTED_SIGNATURES: WebPkiSupportedAlgorithms = WebPkiSupportedAlgorithms {
    all: &[&ECDSA_P256_SHA256],
    mapping: &[(
        SignatureScheme::ECDSA_NISTP256_SHA256,
        &[&ECDSA_P256_SHA256],
    )],
};

/// Laedt den privaten Schluessel des Servers; nur ECDSA P-256 (PKCS #8 oder SEC1).
#[derive(Debug)]
pub(crate) struct KeyLoader;

impl KeyProvider for KeyLoader {
    fn load_private_key(
        &self,
        key_der: PrivateKeyDer<'static>,
    ) -> Result<Arc<dyn SigningKey>, Error> {
        let key = match &key_der {
            PrivateKeyDer::Pkcs8(der) => {
                EcdsaSigningKey::from_pkcs8_der(der.secret_pkcs8_der()).ok()
            }
            PrivateKeyDer::Sec1(der) => p256::SecretKey::from_sec1_der(der.secret_sec1_der())
                .ok()
                .map(EcdsaSigningKey::from),
            _ => None,
        };
        key.map(|key| Arc::new(EcdsaKey(key)) as Arc<dyn SigningKey>)
            .ok_or_else(|| Error::General("nur ECDSA-P-256-Schluessel werden unterstuetzt".into()))
    }
}

struct EcdsaKey(EcdsaSigningKey);

/// Der private Schluessel gehoert nicht in Logs.
impl fmt::Debug for EcdsaKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EcdsaKey(P-256)")
    }
}

impl SigningKey for EcdsaKey {
    fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn Signer>> {
        offered
            .contains(&SignatureScheme::ECDSA_NISTP256_SHA256)
            .then(|| Box::new(EcdsaSigner(self.0.clone())) as Box<dyn Signer>)
    }

    fn algorithm(&self) -> SignatureAlgorithm {
        SignatureAlgorithm::ECDSA
    }
}

struct EcdsaSigner(EcdsaSigningKey);

impl fmt::Debug for EcdsaSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EcdsaSigner(P-256)")
    }
}

impl Signer for EcdsaSigner {
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, Error> {
        // TLS erwartet ECDSA-Signaturen DER-kodiert (RFC 8446, 4.2.3).
        let signature: Signature = self.0.sign(message);
        Ok(signature.to_der().as_bytes().to_vec())
    }

    fn scheme(&self) -> SignatureScheme {
        SignatureScheme::ECDSA_NISTP256_SHA256
    }
}

#[derive(Debug)]
struct EcdsaP256Sha256;

impl SignatureVerificationAlgorithm for EcdsaP256Sha256 {
    fn verify_signature(
        &self,
        public_key: &[u8],
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), InvalidSignature> {
        let key = VerifyingKey::from_sec1_bytes(public_key).map_err(|_| InvalidSignature)?;
        let signature = Signature::from_der(signature).map_err(|_| InvalidSignature)?;
        key.verify(message, &signature)
            .map_err(|_| InvalidSignature)
    }

    fn public_key_alg_id(&self) -> AlgorithmIdentifier {
        alg_id::ECDSA_P256
    }

    fn signature_alg_id(&self) -> AlgorithmIdentifier {
        alg_id::ECDSA_SHA256
    }
}

#[cfg(test)]
mod tests {
    use p256::pkcs8::EncodePrivateKey;
    use rustls::pki_types::{PrivatePkcs1KeyDer, PrivatePkcs8KeyDer};

    use super::*;

    fn new_key() -> EcdsaSigningKey {
        EcdsaSigningKey::random(&mut rand_core::OsRng)
    }

    fn load(key: &EcdsaSigningKey) -> Arc<dyn SigningKey> {
        let der = key.to_pkcs8_der().unwrap();
        KEY_LOADER
            .load_private_key(PrivatePkcs8KeyDer::from(der.as_bytes().to_vec()).into())
            .unwrap()
    }

    #[test]
    fn signatures_verify_with_the_public_key() {
        let key = new_key();
        let signer = load(&key)
            .choose_scheme(&[
                SignatureScheme::ED25519,
                SignatureScheme::ECDSA_NISTP256_SHA256,
            ])
            .unwrap();
        assert_eq!(signer.scheme(), SignatureScheme::ECDSA_NISTP256_SHA256);

        let signature = signer.sign(b"handschlag").unwrap();
        let public = key.verifying_key().to_encoded_point(false);
        assert!(
            ECDSA_P256_SHA256
                .verify_signature(public.as_bytes(), b"handschlag", &signature)
                .is_ok()
        );
        assert!(
            ECDSA_P256_SHA256
                .verify_signature(public.as_bytes(), b"anderes", &signature)
                .is_err()
        );
        assert!(
            ECDSA_P256_SHA256
                .verify_signature(public.as_bytes(), b"handschlag", b"kein der")
                .is_err()
        );
        assert!(
            ECDSA_P256_SHA256
                .verify_signature(b"kein punkt", b"handschlag", &signature)
                .is_err()
        );
    }

    #[test]
    fn offers_nothing_without_p256() {
        assert!(
            load(&new_key())
                .choose_scheme(&[SignatureScheme::ED25519])
                .is_none()
        );
        assert_eq!(load(&new_key()).algorithm(), SignatureAlgorithm::ECDSA);
    }

    #[test]
    fn loads_sec1_and_rejects_other_keys() {
        let secret = p256::SecretKey::random(&mut rand_core::OsRng);
        let sec1 = secret.to_sec1_der().unwrap();
        let sec1 = rustls::pki_types::PrivateSec1KeyDer::from(sec1.to_vec());
        assert!(KEY_LOADER.load_private_key(sec1.into()).is_ok());

        let rsa = PrivatePkcs1KeyDer::from(vec![0x30, 0x00]);
        assert!(KEY_LOADER.load_private_key(rsa.into()).is_err());
        let garbage = PrivatePkcs8KeyDer::from(vec![1, 2, 3]);
        assert!(KEY_LOADER.load_private_key(garbage.into()).is_err());
    }

    #[test]
    fn debug_hides_the_key() {
        let text = format!("{:?}", load(&new_key()));
        assert_eq!(text, "EcdsaKey(P-256)");
    }

    #[test]
    fn names_its_algorithm_identifiers() {
        assert_eq!(ECDSA_P256_SHA256.public_key_alg_id(), alg_id::ECDSA_P256);
        assert_eq!(ECDSA_P256_SHA256.signature_alg_id(), alg_id::ECDSA_SHA256);
    }
}
