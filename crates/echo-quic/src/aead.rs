//! AES-128-GCM: als TLS-1.3-Satzverschluesselung fuer rustls und als
//! Paket- und Header-Schutz fuer QUIC (RFC 9001, Abschnitt 5).
//!
//! In QUIC verschluesselt rustls keine TLS-Saetze; die Satzverschluesselung
//! gehoert trotzdem zur Cipher-Suite und ist darum vollstaendig da.

use aes::Aes128;
use aes::cipher::BlockEncrypt;
use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::{AeadInPlace, Aes128Gcm, KeyInit, Nonce as GcmNonce, Tag as GcmTag};
use rustls::crypto::cipher::{
    AeadKey, InboundOpaqueMessage, InboundPlainMessage, Iv, MessageDecrypter, MessageEncrypter,
    Nonce, OutboundOpaqueMessage, OutboundPlainMessage, PrefixedPayload, Tls13AeadAlgorithm,
    UnsupportedOperationError, make_tls13_aad,
};
use rustls::{ConnectionTrafficSecrets, ContentType, Error, ProtocolVersion, quic};

const KEY_LEN: usize = 16;
const TAG_LEN: usize = 16;
/// Header-Schutz nimmt 16 Byte des geschuetzten Inhalts als Probe (RFC 9001, 5.4.2).
const SAMPLE_LEN: usize = 16;

/// RFC 9001, Anhang B.1: Grenzen fuer AEAD_AES_128_GCM.
const QUIC_CONFIDENTIALITY_LIMIT: u64 = 1 << 23;
const QUIC_INTEGRITY_LIMIT: u64 = 1 << 52;

pub(crate) static AES_128_GCM: Aes128GcmAead = Aes128GcmAead;
pub(crate) static QUIC_AES_128_GCM: QuicAes128Gcm = QuicAes128Gcm;

/// rustls ruft das nur mit `key_len()` Bytes auf; `from_slice` prueft das ohnehin.
fn gcm(key: &AeadKey) -> Aes128Gcm {
    Aes128Gcm::new(GenericArray::from_slice(key.as_ref()))
}

pub(crate) struct Aes128GcmAead;

impl Tls13AeadAlgorithm for Aes128GcmAead {
    fn encrypter(&self, key: AeadKey, iv: Iv) -> Box<dyn MessageEncrypter> {
        Box::new(RecordEncrypter {
            cipher: gcm(&key),
            iv,
        })
    }

    fn decrypter(&self, key: AeadKey, iv: Iv) -> Box<dyn MessageDecrypter> {
        Box::new(RecordDecrypter {
            cipher: gcm(&key),
            iv,
        })
    }

    fn key_len(&self) -> usize {
        KEY_LEN
    }

    fn extract_keys(
        &self,
        key: AeadKey,
        iv: Iv,
    ) -> Result<ConnectionTrafficSecrets, UnsupportedOperationError> {
        Ok(ConnectionTrafficSecrets::Aes128Gcm { key, iv })
    }
}

struct RecordEncrypter {
    cipher: Aes128Gcm,
    iv: Iv,
}

impl MessageEncrypter for RecordEncrypter {
    fn encrypt(
        &mut self,
        msg: OutboundPlainMessage<'_>,
        seq: u64,
    ) -> Result<OutboundOpaqueMessage, Error> {
        let total_len = self.encrypted_payload_len(msg.payload.len());
        let mut payload = PrefixedPayload::with_capacity(total_len);
        payload.extend_from_chunks(&msg.payload);
        payload.extend_from_slice(&msg.typ.to_array());

        let nonce = Nonce::new(&self.iv, seq).0;
        let tag = self
            .cipher
            .encrypt_in_place_detached(
                GcmNonce::from_slice(&nonce),
                &make_tls13_aad(total_len),
                payload.as_mut(),
            )
            .map_err(|_| Error::EncryptError)?;
        payload.extend_from_slice(&tag);

        // TLS 1.3 schreibt in jeden Satz die alte Version 1.2 (RFC 8446, 5.1).
        Ok(OutboundOpaqueMessage::new(
            ContentType::ApplicationData,
            ProtocolVersion::TLSv1_2,
            payload,
        ))
    }

    fn encrypted_payload_len(&self, payload_len: usize) -> usize {
        payload_len + 1 + TAG_LEN
    }
}

struct RecordDecrypter {
    cipher: Aes128Gcm,
    iv: Iv,
}

impl MessageDecrypter for RecordDecrypter {
    fn decrypt<'a>(
        &mut self,
        mut msg: InboundOpaqueMessage<'a>,
        seq: u64,
    ) -> Result<InboundPlainMessage<'a>, Error> {
        let payload = &mut msg.payload;
        let plain_len = payload
            .len()
            .checked_sub(TAG_LEN)
            .ok_or(Error::DecryptError)?;

        let nonce = Nonce::new(&self.iv, seq).0;
        let aad = make_tls13_aad(payload.len());
        let (plain, tag) = payload.split_at_mut(plain_len);
        self.cipher
            .decrypt_in_place_detached(
                GcmNonce::from_slice(&nonce),
                &aad,
                plain,
                GcmTag::from_slice(tag),
            )
            .map_err(|_| Error::DecryptError)?;

        payload.truncate(plain_len);
        msg.into_tls13_unpadded_message()
    }
}

pub(crate) struct QuicAes128Gcm;

impl quic::Algorithm for QuicAes128Gcm {
    fn packet_key(&self, key: AeadKey, iv: Iv) -> Box<dyn quic::PacketKey> {
        Box::new(PacketKey {
            cipher: gcm(&key),
            iv,
        })
    }

    fn header_protection_key(&self, key: AeadKey) -> Box<dyn quic::HeaderProtectionKey> {
        Box::new(HeaderProtectionKey(Aes128::new(GenericArray::from_slice(
            key.as_ref(),
        ))))
    }

    fn aead_key_len(&self) -> usize {
        KEY_LEN
    }
}

/// Schuetzt den Inhalt eines QUIC-Pakets; die Paketnummer macht die Nonce.
struct PacketKey {
    cipher: Aes128Gcm,
    iv: Iv,
}

impl quic::PacketKey for PacketKey {
    fn encrypt_in_place(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<quic::Tag, Error> {
        let nonce = Nonce::new(&self.iv, packet_number).0;
        let tag = self
            .cipher
            .encrypt_in_place_detached(GcmNonce::from_slice(&nonce), header, payload)
            .map_err(|_| Error::EncryptError)?;
        Ok(quic::Tag::from(tag.as_slice()))
    }

    fn decrypt_in_place<'a>(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &'a mut [u8],
    ) -> Result<&'a [u8], Error> {
        let plain_len = payload
            .len()
            .checked_sub(TAG_LEN)
            .ok_or(Error::DecryptError)?;
        let nonce = Nonce::new(&self.iv, packet_number).0;
        let (plain, tag) = payload.split_at_mut(plain_len);
        self.cipher
            .decrypt_in_place_detached(
                GcmNonce::from_slice(&nonce),
                header,
                plain,
                GcmTag::from_slice(tag),
            )
            .map_err(|_| Error::DecryptError)?;
        Ok(&payload[..plain_len])
    }

    fn tag_len(&self) -> usize {
        TAG_LEN
    }

    fn confidentiality_limit(&self) -> u64 {
        QUIC_CONFIDENTIALITY_LIMIT
    }

    fn integrity_limit(&self) -> u64 {
        QUIC_INTEGRITY_LIMIT
    }
}

/// Verdeckt erstes Byte und Paketnummer mit AES-ECB ueber eine Probe des
/// geschuetzten Inhalts (RFC 9001, 5.4.1 und 5.4.3).
struct HeaderProtectionKey(Aes128);

impl HeaderProtectionKey {
    fn mask(&self, sample: &[u8]) -> Result<[u8; SAMPLE_LEN], Error> {
        let sample: [u8; SAMPLE_LEN] = sample
            .try_into()
            .map_err(|_| Error::General("sample of invalid length".into()))?;
        let mut block = GenericArray::from(sample);
        self.0.encrypt_block(&mut block);
        Ok(block.into())
    }

    fn xor_in_place(
        &self,
        sample: &[u8],
        first: &mut u8,
        packet_number: &mut [u8],
        masked: bool,
    ) -> Result<(), Error> {
        const LONG_HEADER_FORM: u8 = 0x80;
        const LONG_HEADER_BITS: u8 = 0x0f;
        const SHORT_HEADER_BITS: u8 = 0x1f;
        const PACKET_NUMBER_LENGTH_BITS: u8 = 0x03;

        // Paketnummern sind hoechstens 4 Byte lang (RFC 9000, 17.1).
        const MAX_PACKET_NUMBER_LEN: usize = 4;

        let mask = self.mask(sample)?;
        let (first_mask, pn_mask) = (mask[0], &mask[1..=MAX_PACKET_NUMBER_LEN]);
        if packet_number.len() > pn_mask.len() {
            return Err(Error::General("packet number too long".into()));
        }

        let bits = if *first & LONG_HEADER_FORM == LONG_HEADER_FORM {
            LONG_HEADER_BITS
        } else {
            SHORT_HEADER_BITS
        };
        // Die Laenge der Paketnummer steht im ersten Byte, und zwar unverdeckt:
        // beim Entfernen erst nach dem Aufdecken, beim Verdecken vorher.
        let first_plain = if masked {
            *first ^ (first_mask & bits)
        } else {
            *first
        };
        let pn_len = usize::from(first_plain & PACKET_NUMBER_LENGTH_BITS) + 1;

        *first ^= first_mask & bits;
        for (byte, m) in packet_number.iter_mut().zip(pn_mask).take(pn_len) {
            *byte ^= m;
        }
        Ok(())
    }
}

impl quic::HeaderProtectionKey for HeaderProtectionKey {
    fn encrypt_in_place(
        &self,
        sample: &[u8],
        first: &mut u8,
        packet_number: &mut [u8],
    ) -> Result<(), Error> {
        self.xor_in_place(sample, first, packet_number, false)
    }

    fn decrypt_in_place(
        &self,
        sample: &[u8],
        first: &mut u8,
        packet_number: &mut [u8],
    ) -> Result<(), Error> {
        self.xor_in_place(sample, first, packet_number, true)
    }

    fn sample_len(&self) -> usize {
        SAMPLE_LEN
    }
}

#[cfg(test)]
mod tests {
    use rustls::Side;
    use rustls::quic::{Algorithm, Keys, Version};

    use super::*;
    use crate::provider::TLS13_AES_128_GCM_SHA256;

    fn bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Die Initial-Schluessel fuer die Ziel-ID aus RFC 9001, Anhang A.
    fn initial(side: Side) -> Keys {
        Keys::initial(
            Version::V1,
            &TLS13_AES_128_GCM_SHA256,
            &QUIC_AES_128_GCM,
            &bytes("8394c8f03e515708"),
            side,
        )
    }

    /// RFC 9001, Anhang A.2: Probe, Maske und geschuetzter Header des Clients.
    #[test]
    fn header_protection_matches_rfc_9001() {
        let key = initial(Side::Client).local.header;
        let sample = bytes("d1b1c98dd7689fb8ec11d242b123dc9b");
        let mut first = 0xc3;
        let mut pn = bytes("00000002");

        key.encrypt_in_place(&sample, &mut first, &mut pn).unwrap();
        assert_eq!(first, 0xc0);
        assert_eq!(pn, bytes("7b9aec34"));

        key.decrypt_in_place(&sample, &mut first, &mut pn).unwrap();
        assert_eq!(first, 0xc3);
        assert_eq!(pn, bytes("00000002"));
        assert_eq!(key.sample_len(), SAMPLE_LEN);
    }

    #[test]
    fn header_protection_rejects_bad_lengths() {
        let key = initial(Side::Client).local.header;
        let mut first = 0x40;
        assert!(
            key.encrypt_in_place(&[0; 8], &mut first, &mut [0; 4])
                .is_err()
        );
        assert!(
            key.encrypt_in_place(&[0; 16], &mut first, &mut [0; 5])
                .is_err()
        );
    }

    #[test]
    fn short_headers_mask_five_bits() {
        let key = initial(Side::Server).local.header;
        let sample = [9; 16];
        let mut first = 0x41;
        let mut pn = [1, 2];
        key.encrypt_in_place(&sample, &mut first, &mut pn).unwrap();
        assert_eq!(first & 0xe0, 0x40);
        key.decrypt_in_place(&sample, &mut first, &mut pn).unwrap();
        assert_eq!((first, pn), (0x41, [1, 2]));
    }

    #[test]
    fn packet_keys_round_trip_and_detect_tampering() {
        let client = initial(Side::Client);
        let server = initial(Side::Server);
        let header = b"kopf";
        let mut payload = b"nutzdaten".to_vec();
        let tag = client
            .local
            .packet
            .encrypt_in_place(42, header, &mut payload)
            .unwrap();
        payload.extend_from_slice(tag.as_ref());

        let remote = &server.remote.packet;
        let mut tampered = payload.clone();
        tampered[0] ^= 1;
        assert!(remote.decrypt_in_place(42, header, &mut tampered).is_err());
        assert!(
            remote
                .decrypt_in_place(43, header, &mut payload.clone())
                .is_err()
        );
        assert!(remote.decrypt_in_place(42, header, &mut [0; 4]).is_err());

        assert_eq!(
            remote.decrypt_in_place(42, header, &mut payload).unwrap(),
            b"nutzdaten"
        );
        assert_eq!(remote.tag_len(), TAG_LEN);
        assert_eq!(remote.confidentiality_limit(), QUIC_CONFIDENTIALITY_LIMIT);
        assert_eq!(remote.integrity_limit(), QUIC_INTEGRITY_LIMIT);
        assert_eq!(QUIC_AES_128_GCM.aead_key_len(), KEY_LEN);
    }
}
