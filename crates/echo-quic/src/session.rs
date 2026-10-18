//! Die Anbindung von rustls an quinn: TLS-Sitzung, Initial-Schluessel und
//! Retry-Integritaet nach RFC 9001.
//!
//! Angepasst aus quinn-proto 0.11 (`src/crypto/rustls.rs`),
//! Copyright (c) 2018 The quinn developers, MIT- oder Apache-2.0-Lizenz.
//! quinn baut diese Datei nur zusammen mit ring oder aws-lc-rs; hier rechnet
//! AES-GCM aus RustCrypto, und Header- und Paketschluessel stecken in eigenen
//! Typen, weil Rust fremde Traits nicht fuer fremde Typen erlaubt.

use std::any::Any;
use std::io;
use std::sync::Arc;

use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::{AeadInPlace, Aes128Gcm, KeyInit, Nonce};
use bytes::BytesMut;
use quinn_proto::crypto::{
    self, CryptoError, ExportKeyingMaterialError, KeyPair, Keys, UnsupportedVersion,
};
use quinn_proto::transport_parameters::TransportParameters;
use quinn_proto::{ConnectError, ConnectionId, Side, TransportError, TransportErrorCode};
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::quic::{self as tls_quic, Connection, KeyChange, Secrets, Suite, Version};

/// Was die Anwendung nach dem Handschlag erfaehrt (`Connection::handshake_data`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeData {
    /// Das ausgehandelte Protokoll (ALPN).
    pub protocol: Option<Vec<u8>>,
    /// Der Servername, den der Client genannt hat; beim Client immer `None`.
    pub server_name: Option<String>,
}

/// RFC 9001, 5.8: feste Schluessel fuer den Integritaetsschutz von Retry.
const RETRY_INTEGRITY_KEY_V1: [u8; 16] = [
    0xbe, 0x0c, 0x69, 0x0b, 0x9f, 0x66, 0x57, 0x5a, 0x1d, 0x76, 0x6b, 0x54, 0xe3, 0x68, 0xc8, 0x4e,
];
const RETRY_INTEGRITY_NONCE_V1: [u8; 12] = [
    0x46, 0x15, 0x99, 0xd3, 0x5d, 0x63, 0x2b, 0xf2, 0x23, 0x98, 0x25, 0xbb,
];
const RETRY_INTEGRITY_KEY_DRAFT: [u8; 16] = [
    0xcc, 0xce, 0x18, 0x7e, 0xd0, 0x9a, 0x09, 0xd0, 0x57, 0x28, 0x15, 0x5a, 0x6c, 0xb9, 0x6b, 0xe1,
];
const RETRY_INTEGRITY_NONCE_DRAFT: [u8; 12] = [
    0xe5, 0x49, 0x30, 0xf9, 0x7f, 0x21, 0x36, 0xf0, 0x53, 0x0a, 0x8c, 0x1c,
];
const RETRY_TAG_LEN: usize = 16;

struct TlsSession {
    version: Version,
    got_handshake_data: bool,
    next_secrets: Option<Secrets>,
    inner: Connection,
    suite: Suite,
}

impl TlsSession {
    fn side(&self) -> Side {
        match self.inner {
            Connection::Client(_) => Side::Client,
            Connection::Server(_) => Side::Server,
        }
    }
}

impl crypto::Session for TlsSession {
    fn initial_keys(&self, dst_cid: &ConnectionId, side: Side) -> Keys {
        initial_keys(self.version, *dst_cid, side, &self.suite)
    }

    fn handshake_data(&self) -> Option<Box<dyn Any>> {
        if !self.got_handshake_data {
            return None;
        }
        Some(Box::new(HandshakeData {
            protocol: self.inner.alpn_protocol().map(<[u8]>::to_vec),
            server_name: match &self.inner {
                Connection::Client(_) => None,
                Connection::Server(session) => session.server_name().map(str::to_owned),
            },
        }))
    }

    /// Der Typ hinter `Any` ist `Vec<CertificateDer<'static>>`.
    fn peer_identity(&self) -> Option<Box<dyn Any>> {
        self.inner
            .peer_certificates()
            .map(|certificates| -> Box<dyn Any> {
                Box::new(
                    certificates
                        .iter()
                        .map(|c| c.clone().into_owned())
                        .collect::<Vec<CertificateDer<'static>>>(),
                )
            })
    }

    fn early_crypto(&self) -> Option<(Box<dyn crypto::HeaderKey>, Box<dyn crypto::PacketKey>)> {
        let keys = self.inner.zero_rtt_keys()?;
        Some((
            Box::new(HeaderKey(keys.header)),
            Box::new(PacketKey(keys.packet)),
        ))
    }

    fn early_data_accepted(&self) -> Option<bool> {
        match &self.inner {
            Connection::Client(session) => Some(session.is_early_data_accepted()),
            Connection::Server(_) => None,
        }
    }

    fn is_handshaking(&self) -> bool {
        self.inner.is_handshaking()
    }

    fn read_handshake(&mut self, buf: &[u8]) -> Result<bool, TransportError> {
        self.inner.read_hs(buf).map_err(|e| {
            if let Some(alert) = self.inner.alert() {
                TransportError {
                    code: TransportErrorCode::crypto(alert.into()),
                    frame: None,
                    reason: e.to_string(),
                }
            } else {
                TransportError {
                    code: TransportErrorCode::PROTOCOL_VIOLATION,
                    frame: None,
                    reason: format!("TLS error: {e}"),
                }
            }
        })?;
        if !self.got_handshake_data {
            // rustls meldet nicht eigens, wann ClientHello bzw. ALPN da sind.
            let have_server_name = match &self.inner {
                Connection::Client(_) => false,
                Connection::Server(session) => session.server_name().is_some(),
            };
            if self.inner.alpn_protocol().is_some() || have_server_name || !self.is_handshaking() {
                self.got_handshake_data = true;
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn transport_parameters(&self) -> Result<Option<TransportParameters>, TransportError> {
        match self.inner.quic_transport_parameters() {
            None => Ok(None),
            Some(buf) => TransportParameters::read(self.side(), &mut io::Cursor::new(buf))
                .map(Some)
                .map_err(Into::into),
        }
    }

    fn write_handshake(&mut self, buf: &mut Vec<u8>) -> Option<Keys> {
        let keys = match self.inner.write_hs(buf)? {
            KeyChange::Handshake { keys } => keys,
            KeyChange::OneRtt { keys, next } => {
                self.next_secrets = Some(next);
                keys
            }
        };
        Some(to_quinn_keys(keys))
    }

    fn next_1rtt_keys(&mut self) -> Option<KeyPair<Box<dyn crypto::PacketKey>>> {
        let keys = self.next_secrets.as_mut()?.next_packet_keys();
        Some(KeyPair {
            local: Box::new(PacketKey(keys.local)),
            remote: Box::new(PacketKey(keys.remote)),
        })
    }

    fn is_valid_retry(&self, orig_dst_cid: &ConnectionId, header: &[u8], payload: &[u8]) -> bool {
        let Some(tag_start) = payload.len().checked_sub(RETRY_TAG_LEN) else {
            return false;
        };
        let Some((key, nonce)) = retry_key(self.version) else {
            return false;
        };
        let mut pseudo_packet = retry_pseudo_packet(orig_dst_cid, header);
        let aad_len = pseudo_packet.len() + tag_start;
        pseudo_packet.extend_from_slice(payload);
        let (aad, tag) = pseudo_packet.split_at_mut(aad_len);
        key.decrypt_in_place_detached(
            Nonce::from_slice(&nonce),
            aad,
            &mut [],
            GenericArray::from_slice(tag),
        )
        .is_ok()
    }

    fn export_keying_material(
        &self,
        output: &mut [u8],
        label: &[u8],
        context: &[u8],
    ) -> Result<(), ExportKeyingMaterialError> {
        self.inner
            .export_keying_material(output, label, Some(context))
            .map(drop)
            .map_err(|_| ExportKeyingMaterialError)
    }
}

/// QUIC-Client-Konfiguration aus einer rustls-Konfiguration mit dem Provider
/// von echo.
pub(crate) struct QuicClientConfig {
    inner: Arc<rustls::ClientConfig>,
    initial: Suite,
}

impl QuicClientConfig {
    pub(crate) fn new(inner: Arc<rustls::ClientConfig>) -> Option<Self> {
        let initial = initial_suite(inner.crypto_provider())?;
        Some(Self { inner, initial })
    }
}

impl crypto::ClientConfig for QuicClientConfig {
    fn start_session(
        self: Arc<Self>,
        version: u32,
        server_name: &str,
        params: &TransportParameters,
    ) -> Result<Box<dyn crypto::Session>, ConnectError> {
        let version = interpret_version(version)?;
        let name = ServerName::try_from(server_name)
            .map_err(|_| ConnectError::InvalidServerName(server_name.into()))?
            .to_owned();
        let connection =
            tls_quic::ClientConnection::new(self.inner.clone(), version, name, to_vec(params))
                .map_err(|_| ConnectError::InvalidServerName(server_name.into()))?;
        Ok(Box::new(TlsSession {
            version,
            got_handshake_data: false,
            next_secrets: None,
            inner: Connection::Client(connection),
            suite: self.initial,
        }))
    }
}

/// QUIC-Server-Konfiguration aus einer rustls-Konfiguration mit dem Provider
/// von echo.
pub(crate) struct QuicServerConfig {
    inner: Arc<rustls::ServerConfig>,
    initial: Suite,
}

impl QuicServerConfig {
    pub(crate) fn new(inner: Arc<rustls::ServerConfig>) -> Option<Self> {
        let initial = initial_suite(inner.crypto_provider())?;
        Some(Self { inner, initial })
    }
}

impl crypto::ServerConfig for QuicServerConfig {
    fn start_session(
        self: Arc<Self>,
        version: u32,
        params: &TransportParameters,
    ) -> Box<dyn crypto::Session> {
        // quinn ruft das erst, nachdem `initial_keys` die Version angenommen hat,
        // und die rustls-Konfiguration hat TLS 1.3 und QUIC-Parameter.
        let version = interpret_version(version).unwrap_or(Version::V1);
        let connection =
            match tls_quic::ServerConnection::new(self.inner.clone(), version, to_vec(params)) {
                Ok(connection) => connection,
                Err(error) => unreachable!("Server-Konfiguration fuer QUIC ungueltig: {error}"),
            };
        Box::new(TlsSession {
            version,
            got_handshake_data: false,
            next_secrets: None,
            inner: Connection::Server(connection),
            suite: self.initial,
        })
    }

    fn initial_keys(
        &self,
        version: u32,
        dst_cid: &ConnectionId,
    ) -> Result<Keys, UnsupportedVersion> {
        let version = interpret_version(version)?;
        Ok(initial_keys(version, *dst_cid, Side::Server, &self.initial))
    }

    fn retry_tag(&self, version: u32, orig_dst_cid: &ConnectionId, packet: &[u8]) -> [u8; 16] {
        // Auch hier hat `initial_keys` die Version schon angenommen.
        let (key, nonce) = interpret_version(version)
            .ok()
            .and_then(retry_key)
            .unwrap_or_else(|| unreachable!("Retry fuer eine unbekannte QUIC-Version"));
        let pseudo_packet = [retry_pseudo_packet(orig_dst_cid, &[]), packet.to_vec()].concat();
        match key.encrypt_in_place_detached(Nonce::from_slice(&nonce), &pseudo_packet, &mut []) {
            Ok(tag) => tag.into(),
            Err(_) => unreachable!("AES-GCM ueber einen leeren Inhalt scheitert nicht"),
        }
    }
}

/// Wie der Header-Schutz von rustls an quinn angeschlossen wird.
struct HeaderKey(Box<dyn tls_quic::HeaderProtectionKey>);

/// Die Grenzen von `pn_offset` und Probe stellt quinn sicher; der Header-Schutz
/// scheitert nur an zu kurzen Proben, und die gibt es hier nicht.
impl crypto::HeaderKey for HeaderKey {
    fn decrypt(&self, pn_offset: usize, packet: &mut [u8]) {
        let (header, sample) = packet.split_at_mut(pn_offset + 4);
        let (first, rest) = header.split_at_mut(1);
        let pn_end = Ord::min(pn_offset + 3, rest.len());
        let sample = &sample[..self.sample_size()];
        if self
            .0
            .decrypt_in_place(sample, &mut first[0], &mut rest[pn_offset - 1..pn_end])
            .is_err()
        {
            unreachable!("quinn liefert immer eine vollstaendige Probe");
        }
    }

    fn encrypt(&self, pn_offset: usize, packet: &mut [u8]) {
        let (header, sample) = packet.split_at_mut(pn_offset + 4);
        let (first, rest) = header.split_at_mut(1);
        let pn_end = Ord::min(pn_offset + 3, rest.len());
        let sample = &sample[..self.sample_size()];
        if self
            .0
            .encrypt_in_place(sample, &mut first[0], &mut rest[pn_offset - 1..pn_end])
            .is_err()
        {
            unreachable!("quinn liefert immer eine vollstaendige Probe");
        }
    }

    fn sample_size(&self) -> usize {
        self.0.sample_len()
    }
}

/// Wie der Paketschutz von rustls an quinn angeschlossen wird.
struct PacketKey(Box<dyn tls_quic::PacketKey>);

impl crypto::PacketKey for PacketKey {
    fn encrypt(&self, packet: u64, buf: &mut [u8], header_len: usize) {
        let (header, payload_tag) = buf.split_at_mut(header_len);
        let (payload, tag_storage) = payload_tag.split_at_mut(payload_tag.len() - self.tag_len());
        match self.0.encrypt_in_place(packet, header, payload) {
            Ok(tag) => tag_storage.copy_from_slice(tag.as_ref()),
            Err(_) => unreachable!("AES-GCM verschluesselt jeden Inhalt"),
        }
    }

    fn decrypt(
        &self,
        packet: u64,
        header: &[u8],
        payload: &mut BytesMut,
    ) -> Result<(), CryptoError> {
        let plain_len = self
            .0
            .decrypt_in_place(packet, header, payload.as_mut())
            .map_err(|_| CryptoError)?
            .len();
        payload.truncate(plain_len);
        Ok(())
    }

    fn tag_len(&self) -> usize {
        self.0.tag_len()
    }

    fn confidentiality_limit(&self) -> u64 {
        self.0.confidentiality_limit()
    }

    fn integrity_limit(&self) -> u64 {
        self.0.integrity_limit()
    }
}

fn to_quinn_keys(keys: tls_quic::Keys) -> Keys {
    Keys {
        header: KeyPair {
            local: Box::new(HeaderKey(keys.local.header)),
            remote: Box::new(HeaderKey(keys.remote.header)),
        },
        packet: KeyPair {
            local: Box::new(PacketKey(keys.local.packet)),
            remote: Box::new(PacketKey(keys.remote.packet)),
        },
    }
}

fn initial_keys(version: Version, dst_cid: ConnectionId, side: Side, suite: &Suite) -> Keys {
    let side = match side {
        Side::Client => rustls::Side::Client,
        Side::Server => rustls::Side::Server,
    };
    to_quinn_keys(suite.keys(&dst_cid, side, version))
}

/// QUIC schuetzt Initial-Pakete immer mit `TLS13_AES_128_GCM_SHA256`.
fn initial_suite(provider: &Arc<rustls::crypto::CryptoProvider>) -> Option<Suite> {
    provider
        .cipher_suites
        .iter()
        .find_map(|suite| match (suite.suite(), suite.tls13()) {
            (rustls::CipherSuite::TLS13_AES_128_GCM_SHA256, Some(tls13)) => tls13.quic_suite(),
            _ => None,
        })
}

fn retry_key(version: Version) -> Option<(Aes128Gcm, [u8; 12])> {
    let (key, nonce) = match version {
        Version::V1 => (RETRY_INTEGRITY_KEY_V1, RETRY_INTEGRITY_NONCE_V1),
        Version::V1Draft => (RETRY_INTEGRITY_KEY_DRAFT, RETRY_INTEGRITY_NONCE_DRAFT),
        _ => return None,
    };
    Some((Aes128Gcm::new(GenericArray::from_slice(&key)), nonce))
}

/// RFC 9001, 5.8: die urspruengliche Ziel-ID mit Laenge vor dem Retry-Paket.
fn retry_pseudo_packet(orig_dst_cid: &ConnectionId, header: &[u8]) -> Vec<u8> {
    let mut pseudo = Vec::with_capacity(1 + orig_dst_cid.len() + header.len());
    // Verbindungs-IDs sind hoechstens 20 Byte lang (RFC 9000, 17.2).
    pseudo.push(u8::try_from(orig_dst_cid.len()).unwrap_or(u8::MAX));
    pseudo.extend_from_slice(orig_dst_cid);
    pseudo.extend_from_slice(header);
    pseudo
}

fn to_vec(params: &TransportParameters) -> Vec<u8> {
    let mut bytes = Vec::new();
    params.write(&mut bytes);
    bytes
}

fn interpret_version(version: u32) -> Result<Version, UnsupportedVersion> {
    match version {
        0xff00_001d..=0xff00_0020 => Ok(Version::V1Draft),
        0x0000_0001 | 0xff00_0021..=0xff00_0022 => Ok(Version::V1),
        _ => Err(UnsupportedVersion),
    }
}

#[cfg(test)]
mod tests {
    use quinn_proto::crypto::ServerConfig as _;

    use super::*;
    use crate::provider::provider;

    fn bytes(hex: &str) -> Vec<u8> {
        let hex: String = hex.split_whitespace().collect();
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    fn suite() -> Suite {
        initial_suite(&Arc::new(provider())).unwrap()
    }

    /// RFC 9001, Anhang A: die Ziel-ID des Clients.
    fn rfc_cid() -> ConnectionId {
        ConnectionId::new(&bytes("8394c8f03e515708"))
    }

    /// RFC 9001, Anhang A.3: das Initial-Paket des Servers, Byte fuer Byte.
    #[test]
    fn server_initial_matches_rfc_9001() {
        let header = bytes("c1000000010008f067a5502a4262b50040750001");
        let payload = bytes(
            "02000000000600405a020000560303ee fce7f7b37ba1d1632e96677825ddf739
             88cfc79825df566dc5430b9a045a1200 130100002e00330024001d00209d3c94
             0d89690b84d08a60993c144eca684d10 81287c834d5311bcf32bb9da1a002b00
             020304",
        );
        let expected = bytes(
            "cf000000010008f067a5502a4262b500 4075c0d95a482cd0991cd25b0aac406a
             5816b6394100f37a1c69797554780bb3 8cc5a99f5ede4cf73c3ec2493a1839b3
             dbcba3f6ea46c5b7684df3548e7ddeb9 c3bf9c73cc3f3bded74b562bfb19fb84
             022f8ef4cdd93795d77d06edbb7aaf2f 58891850abbdca3d20398c276456cbc4
             2158407dd074ee",
        );

        let server = initial_keys(Version::V1, rfc_cid(), Side::Server, &suite());
        let mut packet = [header.clone(), payload.clone(), vec![0; 16]].concat();
        server.packet.local.encrypt(1, &mut packet, header.len());
        // Die Paketnummer beginnt hinter dem Laengenfeld: Byte 18.
        server.header.local.encrypt(18, &mut packet);
        assert_eq!(packet, expected);

        let client = initial_keys(Version::V1, rfc_cid(), Side::Client, &suite());
        client.header.remote.decrypt(18, &mut packet);
        assert_eq!(&packet[..header.len()], &header[..]);
        let mut sealed = BytesMut::from(&packet[header.len()..]);
        client
            .packet
            .remote
            .decrypt(1, &header, &mut sealed)
            .unwrap();
        assert_eq!(&sealed[..], &payload[..]);
    }

    /// RFC 9001, Anhang A.4: das Retry-Paket mit seinem Integritaets-Tag.
    #[test]
    fn retry_tag_matches_rfc_9001() {
        let retry = bytes(
            "ff000000010008f067a5502a4262b574 6f6b656e04a265ba2eff4d829058fb3f
             0f2496ba",
        );
        let (packet, tag) = retry.split_at(retry.len() - RETRY_TAG_LEN);
        let config = QuicServerConfig::new(Arc::new(server_tls())).unwrap();
        assert_eq!(config.retry_tag(1, &rfc_cid(), packet), tag);
    }

    #[test]
    fn rejects_unknown_versions() {
        assert!(interpret_version(0x0000_0002).is_err());
        assert!(matches!(
            interpret_version(0xff00_001d),
            Ok(Version::V1Draft)
        ));
        let config = QuicServerConfig::new(Arc::new(server_tls())).unwrap();
        assert!(config.initial_keys(0x0000_0002, &rfc_cid()).is_err());
        assert!(config.initial_keys(1, &rfc_cid()).is_ok());
    }

    #[test]
    fn packet_keys_reject_tampering() {
        let keys = initial_keys(Version::V1, rfc_cid(), Side::Client, &suite());
        let header = [0xc3, 1, 2, 3];
        let mut packet = [header.to_vec(), b"daten".to_vec(), vec![0; 16]].concat();
        keys.packet.local.encrypt(5, &mut packet, header.len());
        let mut sealed = BytesMut::from(&packet[header.len()..]);
        sealed[0] ^= 1;
        assert!(keys.packet.local.decrypt(5, &header, &mut sealed).is_err());
        assert_eq!(keys.packet.local.tag_len(), 16);
        assert_eq!(keys.packet.local.confidentiality_limit(), 1 << 23);
        assert_eq!(keys.packet.local.integrity_limit(), 1 << 52);
        assert_eq!(keys.header.local.sample_size(), 16);
    }

    fn server_tls() -> rustls::ServerConfig {
        let certificate = crate::HostCertificate::generate("test").unwrap();
        rustls::ServerConfig::builder_with_provider(Arc::new(provider()))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![certificate.certificate()], certificate.key())
            .unwrap()
    }
}
