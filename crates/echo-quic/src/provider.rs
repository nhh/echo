//! Der rustls-Provider: eine Cipher-Suite, eine Gruppe, ein
//! Signaturverfahren. `TLS13_AES_128_GCM_SHA256` muss es sein, weil QUIC
//! damit seine Initial-Pakete schuetzt (RFC 9001, 5.2).

use rustls::crypto::tls13::HkdfUsingHmac;
use rustls::crypto::{CryptoProvider, GetRandomFailed, SecureRandom};
use rustls::{CipherSuite, CipherSuiteCommon, SupportedCipherSuite, Tls13CipherSuite};

use crate::aead::{AES_128_GCM, QUIC_AES_128_GCM};
use crate::digest::{HMAC_SHA256, SHA256};
use crate::kx::X25519;
use crate::sign::{KEY_LOADER, SUPPORTED_SIGNATURES};

/// RFC 8446, 5.5: so viele Saetze vertraegt ein AES-GCM-Schluessel.
const TLS_CONFIDENTIALITY_LIMIT: u64 = 1 << 24;

pub(crate) static TLS13_AES_128_GCM_SHA256: Tls13CipherSuite = Tls13CipherSuite {
    common: CipherSuiteCommon {
        suite: CipherSuite::TLS13_AES_128_GCM_SHA256,
        hash_provider: &SHA256,
        confidentiality_limit: TLS_CONFIDENTIALITY_LIMIT,
    },
    hkdf_provider: &HkdfUsingHmac(&HMAC_SHA256),
    aead_alg: &AES_128_GCM,
    quic: Some(&QUIC_AES_128_GCM),
};

static RANDOM: OsRandom = OsRandom;

/// Der Provider, den echo fuer jede TLS-Verbindung nimmt.
#[must_use]
pub fn provider() -> CryptoProvider {
    CryptoProvider {
        cipher_suites: vec![SupportedCipherSuite::Tls13(&TLS13_AES_128_GCM_SHA256)],
        kx_groups: vec![&X25519],
        signature_verification_algorithms: SUPPORTED_SIGNATURES,
        secure_random: &RANDOM,
        key_provider: &KEY_LOADER,
    }
}

#[derive(Debug)]
struct OsRandom;

impl SecureRandom for OsRandom {
    fn fill(&self, buf: &mut [u8]) -> Result<(), GetRandomFailed> {
        getrandom::getrandom(buf).map_err(|_| GetRandomFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offers_exactly_what_echo_needs() {
        let provider = provider();
        assert_eq!(provider.cipher_suites.len(), 1);
        assert_eq!(
            provider.cipher_suites[0].suite(),
            CipherSuite::TLS13_AES_128_GCM_SHA256
        );
        assert_eq!(provider.kx_groups.len(), 1);
        assert!(!provider.fips());
    }

    #[test]
    fn random_fills_the_buffer() {
        let mut first = [0u8; 32];
        let mut second = [0u8; 32];
        RANDOM.fill(&mut first).unwrap();
        RANDOM.fill(&mut second).unwrap();
        assert_ne!(first, second);
    }
}
