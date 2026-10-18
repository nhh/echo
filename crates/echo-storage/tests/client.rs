//! Die Datei des Clients: Einstellungen, Favoriten, bekannte Server.

// Hilfsfunktionen ausserhalb von #[test] zaehlen fuer clippy nicht als Test.
#![allow(clippy::unwrap_used)]

use echo_storage::{ClientDatabase, Favorite, HostVerdict, StorageError};
use tempfile::TempDir;

fn client() -> (TempDir, ClientDatabase) {
    let dir = TempDir::new().unwrap();
    let db = ClientDatabase::open(&dir.path().join("echo").join("client.sqlite")).unwrap();
    (dir, db)
}

fn favorite(name: &str, address: &str) -> Favorite {
    Favorite {
        name: name.into(),
        address: address.into(),
        nickname: "Alice".into(),
        server_password: Some("geheim".into()),
        default_channel: None,
        default_channel_password: None,
    }
}

#[test]
fn settings_start_empty_and_keep_the_last_document() {
    let (_dir, db) = client();
    assert_eq!(db.settings_json().unwrap(), None);
    db.save_settings_json(r#"{"volume":1}"#).unwrap();
    db.save_settings_json(r#"{"volume":2}"#).unwrap();
    assert_eq!(
        db.settings_json().unwrap().as_deref(),
        Some(r#"{"volume":2}"#)
    );
}

#[test]
fn favorites_keep_their_order_and_replace_by_address() {
    let (_dir, db) = client();
    db.save_favorite(&favorite("Eins", "eins.example:9988"))
        .unwrap();
    db.save_favorite(&favorite("Zwei", "zwei.example")).unwrap();
    db.save_favorite(&favorite("Eins neu", "EINS.example:9988"))
        .unwrap();

    let names: Vec<_> = db
        .favorites()
        .unwrap()
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(names, ["Eins neu", "Zwei"]);

    db.remove_favorite("Zwei.Example").unwrap();
    let left = db.favorites().unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].server_password.as_deref(), Some("geheim"));
}

#[test]
fn hosts_are_trusted_on_first_use() {
    let (_dir, db) = client();
    assert_eq!(
        db.check_host("ts.example", "SHA256:a").unwrap(),
        HostVerdict::New
    );

    db.remember_host("ts.example", "SHA256:a").unwrap();
    assert_eq!(
        db.check_host("TS.example", "SHA256:a").unwrap(),
        HostVerdict::Known
    );
    assert_eq!(
        db.check_host("ts.example", "SHA256:b").unwrap(),
        HostVerdict::Changed
    );

    // Der Nutzer nimmt den neuen Schluessel bewusst an.
    db.remember_host("ts.example", "SHA256:b").unwrap();
    assert_eq!(
        db.check_host("ts.example", "SHA256:b").unwrap(),
        HostVerdict::Known
    );

    db.forget_host("ts.example").unwrap();
    assert_eq!(db.host_fingerprint("ts.example").unwrap(), None);
}

#[test]
fn reopening_keeps_everything_and_newer_files_are_refused() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("client.sqlite");
    {
        let db = ClientDatabase::open(&path).unwrap();
        db.remember_host("a", "SHA256:a").unwrap();
    }
    let db = ClientDatabase::open(&path).unwrap();
    assert_eq!(
        db.host_fingerprint("a").unwrap().as_deref(),
        Some("SHA256:a")
    );
    drop(db);

    let raw = echo_storage::Database::open(&path).unwrap();
    raw.execute("PRAGMA user_version=9", &[]).unwrap();
    drop(raw);
    assert!(matches!(
        ClientDatabase::open(&path),
        Err(StorageError::NewerSchema { version: 9, .. })
    ));
}
