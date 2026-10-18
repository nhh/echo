//! Das Zertifikat des Servers: selbstsigniert mit einem ECDSA-P-256-Schluessel,
//! zehn Jahre gueltig. Keine CA: der Client merkt sich beim ersten Verbinden
//! den Fingerabdruck (known_hosts, wie SSH). Liegt als zwei PEM-Dateien vor,
//! genau wie beim C#-Vorgaenger, damit gemerkte Fingerabdruecke gueltig bleiben.

use std::fmt;
use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, SystemTime};

use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use p256::ecdsa::{DerSignature, SigningKey};
use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};
use x509_cert::builder::{Builder, CertificateBuilder, Profile};
use x509_cert::der::{Decode, Encode, EncodePem};
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::SubjectPublicKeyInfoOwned;
use x509_cert::time::{Time, Validity};

use crate::ConfigError;

const VALID_FOR: Duration = Duration::from_hours(10 * 365 * 24);
/// Gegen Uhren, die etwas nachgehen.
const BACKDATE: Duration = Duration::from_hours(24);
const SERIAL_LEN: usize = 16;
const FALLBACK_NAME: &str = "echo";

/// Zertifikat und privater Schluessel des Servers.
pub struct HostCertificate {
    certificate: CertificateDer<'static>,
    key: PrivatePkcs8KeyDer<'static>,
}

/// Der private Schluessel gehoert nicht in Logs.
impl fmt::Debug for HostCertificate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostCertificate")
            .field("fingerprint", &self.fingerprint())
            .finish_non_exhaustive()
    }
}

impl HostCertificate {
    /// Erzeugt ein neues Zertifikat auf den Namen des Servers.
    pub fn generate(server_name: &str) -> Result<Self, ConfigError> {
        let key = SigningKey::random(&mut rand_core::OsRng);
        let subject =
            Name::from_str(&format!("CN={}", sanitize(server_name))).map_err(certificate_error)?;
        let public_key =
            SubjectPublicKeyInfoOwned::from_key(*key.verifying_key()).map_err(certificate_error)?;
        let builder = CertificateBuilder::new(
            Profile::Leaf {
                issuer: subject.clone(),
                enable_key_agreement: false,
                enable_key_encipherment: false,
            },
            random_serial()?,
            validity()?,
            subject,
            public_key,
            &key,
        )
        .map_err(certificate_error)?;
        let certificate = builder
            .build::<DerSignature>()
            .map_err(certificate_error)?
            .to_der()
            .map_err(certificate_error)?;
        let key = key.to_pkcs8_der().map_err(certificate_error)?;
        Ok(Self {
            certificate: CertificateDer::from(certificate),
            key: PrivatePkcs8KeyDer::from(key.as_bytes().to_vec()),
        })
    }

    /// Liest Zertifikat und Schluessel aus zwei PEM-Dateien.
    pub fn load(certificate: &Path, key: &Path) -> Result<Self, ConfigError> {
        let certificate = CertificateDer::from_pem_file(certificate).map_err(|e| pem_error(&e))?;
        let PrivateKeyDer::Pkcs8(key) =
            PrivateKeyDer::from_pem_file(key).map_err(|e| pem_error(&e))?
        else {
            return Err(ConfigError::Certificate(
                "Schluessel ist kein PKCS #8".into(),
            ));
        };
        Ok(Self { certificate, key })
    }

    /// Liest die PEM-Dateien oder legt sie mit einem neuen Zertifikat an.
    pub fn load_or_generate(
        certificate: &Path,
        key: &Path,
        server_name: &str,
    ) -> Result<Self, ConfigError> {
        if certificate.exists() && key.exists() {
            return Self::load(certificate, key);
        }
        let generated = Self::generate(server_name)?;
        generated.save(certificate, key)?;
        Ok(generated)
    }

    /// Schreibt Zertifikat und Schluessel als PEM.
    pub fn save(&self, certificate: &Path, key: &Path) -> Result<(), ConfigError> {
        for path in [certificate, key] {
            if let Some(folder) = path.parent() {
                std::fs::create_dir_all(folder)?;
            }
        }
        let certificate_pem = x509_cert::Certificate::from_der(&self.certificate)
            .and_then(|c| c.to_pem(LineEnding::LF))
            .map_err(certificate_error)?;
        std::fs::write(certificate, certificate_pem)?;
        let key_pem = SigningKey::from_pkcs8_der(self.key.secret_pkcs8_der())
            .map_err(certificate_error)?
            .to_pkcs8_pem(LineEnding::LF)
            .map_err(certificate_error)?;
        std::fs::write(key, key_pem.as_bytes())?;
        Ok(())
    }

    /// Der Fingerabdruck, den Clients sich merken.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        fingerprint(&self.certificate)
    }

    pub(crate) fn certificate(&self) -> CertificateDer<'static> {
        self.certificate.clone()
    }

    pub(crate) fn key(&self) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(self.key.clone_key())
    }
}

/// SHA-256 ueber das DER-Zertifikat, Base64 ohne Auffuellung, mit Praefix
/// `SHA256:` — dasselbe Format wie beim C#-Vorgaenger und wie bei SSH.
#[must_use]
pub fn fingerprint(certificate_der: &[u8]) -> String {
    format!(
        "SHA256:{}",
        STANDARD_NO_PAD.encode(Sha256::digest(certificate_der))
    )
}

/// Nur Zeichen, die in einem CN niemanden stoeren; wie im C#-Vorgaenger.
fn sanitize(name: &str) -> String {
    let clean: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
        .collect();
    let clean = clean.trim();
    if clean.is_empty() {
        FALLBACK_NAME.to_owned()
    } else {
        clean.to_owned()
    }
}

fn validity() -> Result<Validity, ConfigError> {
    let now = SystemTime::now();
    let time = |at: SystemTime| Time::try_from(at).map_err(certificate_error);
    Ok(Validity {
        not_before: time(now - BACKDATE)?,
        not_after: time(now + VALID_FOR)?,
    })
}

fn random_serial() -> Result<SerialNumber, ConfigError> {
    let mut serial = [0u8; SERIAL_LEN];
    getrandom::getrandom(&mut serial)?;
    // Seriennummern sind positiv (RFC 5280, 4.1.2.2).
    serial[0] &= 0x7f;
    SerialNumber::new(&serial).map_err(certificate_error)
}

fn certificate_error(error: impl fmt::Display) -> ConfigError {
    ConfigError::Certificate(error.to_string())
}

fn pem_error(error: &rustls::pki_types::pem::Error) -> ConfigError {
    ConfigError::Certificate(format!("PEM: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_sanitized_like_in_csharp() {
        assert_eq!(sanitize("  Mein Server! <3 "), "Mein Server 3");
        assert_eq!(sanitize("öffentlich.de"), "öffentlich.de");
        assert_eq!(sanitize("!!!"), "echo");
    }

    #[test]
    fn fingerprint_is_sha256_base64_without_padding() {
        // SHA-256 von "" wie im C#-Vorgaenger.
        assert_eq!(
            fingerprint(b""),
            "SHA256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU"
        );
    }

    #[test]
    fn saved_certificates_load_with_the_same_fingerprint() {
        let dir = std::env::temp_dir().join(format!("echo-quic-{}", std::process::id()));
        let (cert, key) = (dir.join("host.crt"), dir.join("host.key"));
        let _ = std::fs::remove_dir_all(&dir);

        let created = HostCertificate::load_or_generate(&cert, &key, "Test").unwrap();
        let loaded = HostCertificate::load_or_generate(&cert, &key, "anders").unwrap();
        assert_eq!(created.fingerprint(), loaded.fingerprint());
        assert!(
            std::fs::read_to_string(&cert)
                .unwrap()
                .starts_with("-----BEGIN CERTIFICATE-----")
        );
        assert!(format!("{created:?}").contains("SHA256:"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_files_that_are_no_pem() {
        let dir = std::env::temp_dir().join(format!("echo-quic-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bad = dir.join("bad.pem");
        std::fs::write(&bad, "kein pem").unwrap();
        assert!(HostCertificate::load(&bad, &bad).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
