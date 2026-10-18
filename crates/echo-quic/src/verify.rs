//! Wie der Client den Server prueft: nicht ueber eine CA, sondern ueber den
//! Fingerabdruck, den die Anwendung nach dem Handschlag mit `known_hosts`
//! vergleicht. Hier wird nur sichergestellt, dass der Server den Schluessel
//! seines Zertifikats wirklich besitzt — die Signatur ueber den Handschlag
//! wird voll geprueft.

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::verify_tls13_signature;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error, PeerIncompatible, SignatureScheme};

use crate::sign::SUPPORTED_SIGNATURES;

#[derive(Debug)]
pub(crate) struct PinnedByApplication;

impl ServerCertVerifier for PinnedByApplication {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        // Vertrauen entsteht ueber den Fingerabdruck in der Anwendung.
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        // QUIC verlangt TLS 1.3 (RFC 9001, 4.2).
        Err(PeerIncompatible::Tls12NotOffered.into())
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, dss, &SUPPORTED_SIGNATURES)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        SUPPORTED_SIGNATURES.supported_schemes()
    }
}
