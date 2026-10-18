//! QUIC fuer echo in reinem Rust (echo-225).
//!
//! quinn bringt seine rustls-Anbindung nur zusammen mit `ring` oder
//! `aws-lc-rs` mit, beide bauen C und Assembler. Darum steht hier beides
//! selbst: ein rustls-Provider aus RustCrypto-Bausteinen mit genau dem, was
//! echo braucht (TLS 1.3, `TLS13_AES_128_GCM_SHA256`, X25519, ECDSA P-256),
//! und die Anbindung an quinn.
//!
//! echo spricht nur mit echo; mehr Algorithmen machen nur mehr Code, den
//! niemand benutzt. Wer den Server ist, entscheidet nicht eine CA, sondern
//! der Fingerabdruck seines Zertifikats (wie `known_hosts` bei SSH, siehe
//! [`server_fingerprint`]).

mod aead;
mod certificate;
mod config;
mod digest;
mod kx;
mod provider;
mod session;
mod sign;
mod tokens;
mod verify;

pub use certificate::{HostCertificate, fingerprint};
pub use config::{ConfigError, client_config, endpoint_config, server_config, server_fingerprint};
pub use provider::provider;
pub use session::HandshakeData;
