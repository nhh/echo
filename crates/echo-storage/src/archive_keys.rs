//! Dateinamen fuer die Archive privater Gespraeche (echo-182). Ein
//! Fingerabdruck ist "SHA256:" plus Base64 — mit '/', '+' und ':' kein
//! Dateiname, und Windows unterscheidet Gross- und Kleinschreibung nicht.
//! Darum steht im Namen der Hash als Hexziffern; umkehrbar, damit der Server
//! aus dem Ordner lesen kann, mit wem es Gespraeche gibt.

use std::fmt::Write;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};

const PREFIX: &str = "SHA256:";
const HASH_BYTES: usize = 32;

/// Hexziffern des Hashes, oder `None`, wenn es kein Fingerabdruck ist.
#[must_use]
pub fn file_key(fingerprint: &str) -> Option<String> {
    let encoded = fingerprint.strip_prefix(PREFIX)?;
    // Der C#-Vorgaenger schreibt ohne Auffuellung, nimmt sie beim Lesen aber hin.
    let bytes = STANDARD_NO_PAD.decode(encoded.trim_end_matches('=')).ok()?;
    (bytes.len() == HASH_BYTES).then(|| to_hex(&bytes))
}

/// Umkehrung von [`file_key`]; `None`, wenn `key` keine 64 Hexziffern sind.
#[must_use]
pub fn fingerprint(key: &str) -> Option<String> {
    if key.len() != 2 * HASH_BYTES || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }

    let bytes = (0..HASH_BYTES)
        .map(|i| u8::from_str_radix(&key[2 * i..2 * i + 2], 16).ok())
        .collect::<Option<Vec<_>>>()?;
    Some(format!(
        "{PREFIX}{}",
        STANDARD.encode(bytes).trim_end_matches('=')
    ))
}

fn to_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(2 * bytes.len()), |mut hex, b| {
            // Schreiben in einen String scheitert nicht.
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SHA-256 von "" und "a", wie sie in den Testdaten des C#-Servers stehen.
    const EMPTY: &str = "SHA256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU";
    const EMPTY_KEY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const A: &str = "SHA256:ypeBEsobvcr6wjGzmiPcTaeG7/gUfE5yuYB3ha/uSLs";
    const A_KEY: &str = "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb";

    #[test]
    fn file_key_matches_the_csharp_names() {
        assert_eq!(file_key(EMPTY).as_deref(), Some(EMPTY_KEY));
        assert_eq!(file_key(A).as_deref(), Some(A_KEY));
    }

    #[test]
    fn fingerprint_reverses_file_key() {
        assert_eq!(fingerprint(EMPTY_KEY).as_deref(), Some(EMPTY));
        assert_eq!(fingerprint(A_KEY).as_deref(), Some(A));
    }

    #[test]
    fn file_key_accepts_padding() {
        assert_eq!(file_key(&format!("{EMPTY}=")).as_deref(), Some(EMPTY_KEY));
    }

    #[test]
    fn file_key_rejects_what_is_no_fingerprint() {
        assert_eq!(
            file_key("47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU"),
            None
        );
        assert_eq!(file_key("SHA256:kein base64!"), None);
        assert_eq!(file_key("SHA256:YWJj"), None);
    }

    #[test]
    fn fingerprint_rejects_what_is_no_key() {
        assert_eq!(fingerprint("abc"), None);
        assert_eq!(fingerprint(&"g".repeat(64)), None);
        assert_eq!(fingerprint(&"ä".repeat(32)), None);
    }

    #[test]
    fn fingerprint_accepts_upper_case_hex() {
        assert_eq!(fingerprint(&A_KEY.to_uppercase()).as_deref(), Some(A));
    }
}
