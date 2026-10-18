//! Die Identitaet eines Nutzers: ein ECDSA-P-256-Schluesselpaar, wie ein
//! SSH-Schluessel als PEM-Datei im Profil (`id_ecdsa`). Der Fingerabdruck
//! (SHA-256 ueber den SubjectPublicKeyInfo, Base64 ohne Auffuellung, mit
//! Praefix wie bei OpenSSH) ist die dauerhafte Nutzer-ID auf dem Server.
//! Dateien und Formate sind die des C#-Vorgaengers.

use std::fmt;
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use hmac::{Hmac, Mac};
use p256::ecdsa::signature::{Signer as _, Verifier as _};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use p256::pkcs8::{
    DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey, LineEnding,
};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::ProtocolError;

const NONCE_BYTES: usize = 24;
const FINGERPRINT_PREFIX: &str = "SHA256:";

pub struct Identity {
    key: SigningKey,
    public_key: String,
    fingerprint: String,
}

/// Der private Schluessel gehoert nicht in Logs.
impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Identity")
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

impl Identity {
    /// Ein neuer Schluessel.
    pub fn generate() -> Result<Self, ProtocolError> {
        Self::from_key(SigningKey::random(&mut rand_core::OsRng))
    }

    /// Liest PKCS #8 (`PRIVATE KEY`) oder SEC1 (`EC PRIVATE KEY`), wie
    /// `ECDsa.ImportFromPem` im C#-Vorgaenger.
    pub fn from_pem(pem: &str) -> Result<Self, ProtocolError> {
        let key = SigningKey::from_pkcs8_pem(pem)
            .or_else(|_| p256::SecretKey::from_sec1_pem(pem).map(SigningKey::from))
            .map_err(|e| ProtocolError::Key(e.to_string()))?;
        Self::from_key(key)
    }

    /// Als PKCS #8, wie der C#-Vorgaenger es schreibt.
    pub fn to_pem(&self) -> Result<String, ProtocolError> {
        self.key
            .to_pkcs8_pem(LineEnding::LF)
            .map(|pem| pem.to_string())
            .map_err(|e| ProtocolError::Key(e.to_string()))
    }

    /// Laedt die Datei oder legt sie mit einem neuen Schluessel an.
    pub fn load_or_create(path: &Path) -> Result<Self, ProtocolError> {
        if path.exists() {
            return Self::from_pem(&std::fs::read_to_string(path)?);
        }
        let identity = Self::generate()?;
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(path, identity.to_pem()?)?;
        Ok(identity)
    }

    /// Base64 des SubjectPublicKeyInfo; so geht er in `Hello`.
    #[must_use]
    pub fn public_key(&self) -> &str {
        &self.public_key
    }

    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// ECDSA mit SHA-256, DER-kodiert, als Base64.
    #[must_use]
    pub fn sign(&self, payload: &[u8]) -> String {
        let signature: Signature = self.key.sign(payload);
        STANDARD.encode(signature.to_der().as_bytes())
    }

    /// Prueft eine Signatur aus `Hello`. Unlesbare Schluessel oder
    /// Signaturen sind einfach ungueltig.
    #[must_use]
    pub fn verify(public_key: &str, payload: &[u8], signature: &str) -> bool {
        let Some(key) = decode_public_key(public_key) else {
            return false;
        };
        let Ok(signature) = STANDARD.decode(signature) else {
            return false;
        };
        Signature::from_der(&signature)
            .is_ok_and(|signature| key.verify(payload, &signature).is_ok())
    }

    /// Der Fingerabdruck zu einem oeffentlichen Schluessel aus `Hello`.
    pub fn fingerprint_of(public_key: &str) -> Result<String, ProtocolError> {
        let der = STANDARD
            .decode(public_key)
            .map_err(|e| ProtocolError::Key(e.to_string()))?;
        Ok(fingerprint_of_der(&der))
    }

    fn from_key(key: SigningKey) -> Result<Self, ProtocolError> {
        let der = key
            .verifying_key()
            .to_public_key_der()
            .map_err(|e| ProtocolError::Key(e.to_string()))?;
        Ok(Self {
            public_key: STANDARD.encode(der.as_bytes()),
            fingerprint: fingerprint_of_der(der.as_bytes()),
            key,
        })
    }
}

fn decode_public_key(public_key: &str) -> Option<VerifyingKey> {
    let der = STANDARD.decode(public_key).ok()?;
    VerifyingKey::from_public_key_der(&der).ok()
}

fn fingerprint_of_der(der: &[u8]) -> String {
    format!(
        "{FINGERPRINT_PREFIX}{}",
        STANDARD_NO_PAD.encode(Sha256::digest(der))
    )
}

/// Was der Client im Handschlag signiert: Nonce, Fingerabdruck des Servers
/// und Name. So gilt die Signatur nur fuer diesen Server und diesen Namen.
#[must_use]
pub fn hello_payload(nonce: &str, host_fingerprint: &str, nickname: &str) -> Vec<u8> {
    format!("echo-hello\n{nonce}\n{host_fingerprint}\n{nickname}").into_bytes()
}

/// Eine frische Nonce fuer `Challenge`.
pub fn new_nonce() -> Result<String, ProtocolError> {
    let mut bytes = [0u8; NONCE_BYTES];
    getrandom::getrandom(&mut bytes).map_err(|e| ProtocolError::Key(e.to_string()))?;
    Ok(STANDARD.encode(bytes))
}

/// Beweist, dass der Client das Serverpasswort kennt, ohne es zu senden:
/// HMAC ueber die Nonce des Servers mit dem Hash des Passworts als
/// Schluessel. Weil die Nonce je Verbindung neu ist, nuetzt ein
/// mitgeschnittener Beweis beim naechsten Mal nichts (echo-37).
#[derive(Debug)]
pub struct PasswordProof;

impl PasswordProof {
    #[must_use]
    pub fn compute(nonce: &str, password: &str) -> String {
        let key = Sha256::digest(password.as_bytes());
        let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(&key) else {
            unreachable!("HMAC nimmt Schluessel jeder Laenge")
        };
        mac.update(format!("echo-password\n{nonce}").as_bytes());
        STANDARD.encode(mac.finalize().into_bytes())
    }

    /// Vergleicht in gleichbleibender Zeit; fehlende oder unlesbare Beweise
    /// stimmen nie.
    #[must_use]
    pub fn matches(given: Option<&str>, expected: &str) -> bool {
        let (Some(given), Ok(expected)) = (given, STANDARD.decode(expected)) else {
            return false;
        };
        STANDARD
            .decode(given)
            .is_ok_and(|given| given.ct_eq(&expected).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_bind_server_and_name() {
        let identity = Identity::generate().unwrap();
        let payload = hello_payload("nonce", "SHA256:server", "Alice");
        let signature = identity.sign(&payload);

        assert!(Identity::verify(
            identity.public_key(),
            &payload,
            &signature
        ));
        let other_server = hello_payload("nonce", "SHA256:anderer", "Alice");
        assert!(!Identity::verify(
            identity.public_key(),
            &other_server,
            &signature
        ));
        assert!(!Identity::verify("kein base64!", &payload, &signature));
        assert!(!Identity::verify(identity.public_key(), &payload, "AAAA"));
        assert!(!Identity::verify(
            identity.public_key(),
            &payload,
            "kein base64!"
        ));
    }

    #[test]
    fn pem_round_trips_with_the_same_fingerprint() {
        let identity = Identity::generate().unwrap();
        let again = Identity::from_pem(&identity.to_pem().unwrap()).unwrap();
        assert_eq!(identity.fingerprint(), again.fingerprint());
        assert_eq!(
            Identity::fingerprint_of(identity.public_key()).unwrap(),
            identity.fingerprint()
        );
        assert!(identity.fingerprint().starts_with("SHA256:"));
        assert!(Identity::from_pem("kein pem").is_err());
        assert!(Identity::fingerprint_of("kein base64!").is_err());
        assert!(!format!("{identity:?}").contains("PRIVATE"));
    }

    #[test]
    fn load_or_create_keeps_the_key() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("profil").join("id_ecdsa");
        let created = Identity::load_or_create(&path).unwrap();
        let loaded = Identity::load_or_create(&path).unwrap();
        assert_eq!(created.fingerprint(), loaded.fingerprint());
    }

    #[test]
    fn password_proofs_match_only_the_right_password() {
        let proof = PasswordProof::compute("nonce", "geheim");
        assert!(PasswordProof::matches(Some(&proof), &proof));
        assert!(!PasswordProof::matches(
            Some(&PasswordProof::compute("nonce", "falsch")),
            &proof
        ));
        assert!(!PasswordProof::matches(
            Some(&PasswordProof::compute("andere", "geheim")),
            &proof
        ));
        assert!(!PasswordProof::matches(None, &proof));
        assert!(!PasswordProof::matches(Some("kein base64!"), &proof));
    }

    #[test]
    fn nonces_are_fresh() {
        assert_ne!(new_nonce().unwrap(), new_nonce().unwrap());
        assert_eq!(
            STANDARD.decode(new_nonce().unwrap()).unwrap().len(),
            NONCE_BYTES
        );
    }
}
