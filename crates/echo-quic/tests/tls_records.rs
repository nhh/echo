//! TLS 1.3 ohne QUIC, ganz im Speicher: prueft Satzverschluesselung,
//! Schluesseltausch und die Signatur des Servers mit dem Provider von echo,
//! unabhaengig von quinn. Laedt das Zertifikat ueber PEM, wie der Server es tut.

// Hilfsfunktionen ausserhalb von #[test] zaehlen fuer clippy nicht als Test.
#![allow(clippy::unwrap_used)]

use std::io::{Read, Write};
use std::sync::Arc;

use echo_quic::{HostCertificate, provider};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{ClientConnection, DigitallySignedStruct, ServerConnection, SignatureScheme};

/// Prueft nur die Signatur, wie der Client von echo.
#[derive(Debug)]
struct SignatureOnly;

impl ServerCertVerifier for SignatureOnly {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("nur TLS 1.3".into()))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn connections() -> (ClientConnection, ServerConnection) {
    let dir = tempfile::TempDir::new().unwrap();
    let (cert_path, key_path) = (dir.path().join("host.crt"), dir.path().join("host.key"));
    HostCertificate::generate("Records")
        .unwrap()
        .save(&cert_path, &key_path)
        .unwrap();
    let cert = CertificateDer::from_pem_file(&cert_path).unwrap();
    let key = PrivateKeyDer::from_pem_file(&key_path).unwrap();

    let server = rustls::ServerConfig::builder_with_provider(Arc::new(provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
    let client = rustls::ClientConfig::builder_with_provider(Arc::new(provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SignatureOnly))
        .with_no_client_auth();

    let name = ServerName::try_from("echo").unwrap();
    (
        ClientConnection::new(Arc::new(client), name).unwrap(),
        ServerConnection::new(Arc::new(server)).unwrap(),
    )
}

/// Schiebt Saetze hin und her, bis nichts mehr zu senden ist.
fn pump(client: &mut ClientConnection, server: &mut ServerConnection) {
    while client.wants_write() || server.wants_write() {
        let mut buf = Vec::new();
        client.write_tls(&mut buf).unwrap();
        server.read_tls(&mut buf.as_slice()).unwrap();
        server.process_new_packets().unwrap();

        let mut buf = Vec::new();
        server.write_tls(&mut buf).unwrap();
        client.read_tls(&mut buf.as_slice()).unwrap();
        client.process_new_packets().unwrap();
    }
}

#[test]
fn application_data_crosses_tls_13_records() {
    let (mut client, mut server) = connections();
    pump(&mut client, &mut server);
    assert!(!client.is_handshaking() && !server.is_handshaking());
    assert_eq!(
        client.negotiated_cipher_suite().unwrap().suite(),
        rustls::CipherSuite::TLS13_AES_128_GCM_SHA256
    );

    client.writer().write_all(b"hallo server").unwrap();
    server.writer().write_all(b"hallo client").unwrap();
    pump(&mut client, &mut server);

    let mut text = [0u8; 12];
    server.reader().read_exact(&mut text).unwrap();
    assert_eq!(&text, b"hallo server");
    client.reader().read_exact(&mut text).unwrap();
    assert_eq!(&text, b"hallo client");
}
