//! Fertige quinn-Konfigurationen fuer Server und Client von echo.

use std::sync::Arc;

use quinn::{ClientConfig, EndpointConfig, ServerConfig};
use rustls::pki_types::CertificateDer;

use crate::HostCertificate;
use crate::provider::provider;
use crate::session::{QuicClientConfig, QuicServerConfig};
use crate::tokens::{ResetKey, TokenKey};
use crate::verify::PinnedByApplication;

/// Was beim Aufsetzen von TLS und QUIC schiefgehen kann.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("TLS: {0}")]
    Tls(#[from] rustls::Error),

    #[error("Zufall nicht verfuegbar: {0}")]
    Random(#[from] getrandom::Error),

    #[error("Zertifikat: {0}")]
    Certificate(String),

    #[error("Datei nicht erreichbar: {0}")]
    Io(#[from] std::io::Error),

    /// Der Provider hat `TLS13_AES_128_GCM_SHA256` nicht, ohne den QUIC
    /// keine Initial-Pakete schuetzen kann.
    #[error("keine Cipher-Suite fuer QUIC-Initial-Pakete")]
    NoInitialSuite,
}

/// Der Endpunkt mit einem zufaelligen Schluessel fuer Stateless Resets.
pub fn endpoint_config() -> Result<EndpointConfig, ConfigError> {
    Ok(EndpointConfig::new(Arc::new(ResetKey::random()?)))
}

/// Der Server mit seinem Zertifikat; `alpn` ist die Protokollkennung von echo.
pub fn server_config(
    certificate: &HostCertificate,
    alpn: &[u8],
) -> Result<ServerConfig, ConfigError> {
    let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_no_client_auth()
        .with_single_cert(vec![certificate.certificate()], certificate.key())?;
    tls.alpn_protocols = vec![alpn.to_vec()];

    let quic = QuicServerConfig::new(Arc::new(tls)).ok_or(ConfigError::NoInitialSuite)?;
    Ok(ServerConfig::new(
        Arc::new(quic),
        Arc::new(TokenKey::random()?),
    ))
}

/// Der Client. Er nimmt jedes Server-Zertifikat an, dessen Schluessel den
/// Handschlag signiert hat; ob es das richtige ist, entscheidet die Anwendung
/// danach ueber [`server_fingerprint`] und `known_hosts`.
pub fn client_config(alpn: &[u8]) -> Result<ClientConfig, ConfigError> {
    let mut tls = rustls::ClientConfig::builder_with_provider(Arc::new(provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedByApplication))
        .with_no_client_auth();
    tls.alpn_protocols = vec![alpn.to_vec()];

    let quic = QuicClientConfig::new(Arc::new(tls)).ok_or(ConfigError::NoInitialSuite)?;
    Ok(ClientConfig::new(Arc::new(quic)))
}

/// Der Fingerabdruck des Server-Zertifikats einer aufgebauten Verbindung.
#[must_use]
pub fn server_fingerprint(connection: &quinn::Connection) -> Option<String> {
    let identity = connection.peer_identity()?;
    let certificates = identity.downcast_ref::<Vec<CertificateDer<'static>>>()?;
    certificates.first().map(|c| crate::fingerprint(c))
}
