//! Eine Identitaet, die der C#-Client geschrieben hat
//! (`tests/fixtures/csharp/identity`): Rust muss sie lesen, denselben
//! Fingerabdruck zeigen, die C#-Signatur annehmen und denselben
//! Passwort-Beweis rechnen.

// Hilfsfunktionen ausserhalb von #[test] zaehlen fuer clippy nicht als Test.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use echo_protocol::{Identity, PasswordProof, hello_payload};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/csharp/identity")
        .join(name)
}

struct Expected {
    fingerprint: String,
    public_key: String,
    signature: String,
    password_proof: String,
}

fn expected() -> Expected {
    let text = std::fs::read_to_string(fixture("expected.txt")).unwrap();
    let mut lines = text.lines().map(str::to_owned);
    Expected {
        fingerprint: lines.next().unwrap(),
        public_key: lines.next().unwrap(),
        signature: lines.next().unwrap(),
        password_proof: lines.next().unwrap(),
    }
}

#[test]
fn reads_the_identity_of_the_csharp_client() {
    let identity = Identity::load_or_create(&fixture("id_ecdsa")).unwrap();
    let expected = expected();
    assert_eq!(identity.fingerprint(), expected.fingerprint);
    assert_eq!(identity.public_key(), expected.public_key);
}

#[test]
fn accepts_signatures_made_in_csharp() {
    let expected = expected();
    let payload = hello_payload("nonce", "SHA256:host", "Alice");
    assert!(Identity::verify(
        &expected.public_key,
        &payload,
        &expected.signature
    ));
    let other = hello_payload("nonce", "SHA256:host", "Mallory");
    assert!(!Identity::verify(
        &expected.public_key,
        &other,
        &expected.signature
    ));
}

#[test]
fn computes_the_same_password_proof() {
    assert_eq!(
        PasswordProof::compute("nonce", "geheim"),
        expected().password_proof
    );
}
