//! Dateibereich, Datenverbindung und Chatbilder mit echten QUIC-Clients.

// Szenarien lesen sich am besten am Stueck.
#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

mod common;

use common::{Login, TestServer, connect, download, settings, token, upload};
use echo_protocol::{ArchiveRef, ChatScope, Message, Role};
use echo_server::ServerSettings;

/// Ein PNG von einem Pixel.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89,
];

fn entries(message: &Message) -> Vec<(String, bool, u64)> {
    match message {
        Message::FileList { entries, .. } => entries
            .iter()
            .map(|e| (e.name.clone(), e.is_folder, e.size))
            .collect(),
        other => panic!("FileList erwartet: {other:?}"),
    }
}

fn list(channel: u64, folder: &str) -> Message {
    Message::ListFiles {
        channel,
        folder: folder.into(),
    }
}

#[tokio::test]
async fn files_go_up_and_come_down_from_any_offset() {
    let server = TestServer::start(settings());
    let mut alice = connect(&server, Login::named("Alice")).await.unwrap();
    let mut bob = connect(&server, Login::named("Bob")).await.unwrap();

    alice
        .ask(Message::CreateFolder {
            channel: 1,
            folder: String::new(),
            name: "Docs".into(),
        })
        .await
        .unwrap();
    let content: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let ticket = alice
        .request(Message::RequestUpload {
            channel: 1,
            folder: "Docs".into(),
            name: "daten.bin".into(),
            size: content.len() as u64,
        })
        .await;
    upload(server.address(), &token(ticket), &content)
        .await
        .unwrap();
    assert!(matches!(
        bob.event(|m| matches!(m, Message::FilesChanged { .. }))
            .await,
        Message::FilesChanged { channel: 1 }
    ));

    let listed = bob.request(list(1, "Docs")).await;
    assert_eq!(entries(&listed), [("daten.bin".to_owned(), false, 300_000)]);
    let Message::FileList {
        entries: files,
        used,
        ..
    } = listed
    else {
        unreachable!()
    };
    assert_eq!(used, 300_000);

    let whole = bob
        .request(Message::RequestDownload {
            file_id: files[0].id.clone(),
            offset: 0,
            stream: false,
        })
        .await;
    assert_eq!(
        download(server.address(), &token(whole)).await.unwrap(),
        content
    );
    let tail = bob
        .request(Message::RequestDownload {
            file_id: files[0].id.clone(),
            offset: 299_990,
            stream: true,
        })
        .await;
    assert_eq!(
        download(server.address(), &token(tail)).await.unwrap(),
        &content[299_990..]
    );
    assert_eq!(
        bob.ask(Message::RequestDownload {
            file_id: files[0].id.clone(),
            offset: 300_001,
            stream: false
        })
        .await,
        Err("Diese Stelle gibt es in der Datei nicht.".into())
    );

    // Ein Token gilt genau einmal.
    let once = bob
        .request(Message::RequestDownload {
            file_id: files[0].id.clone(),
            offset: 0,
            stream: false,
        })
        .await;
    let once = token(once);
    download(server.address(), &once).await.unwrap();
    assert_eq!(
        download(server.address(), &once).await.err().as_deref(),
        Some("Die Eintrittskarte ist abgelaufen.")
    );
}

#[tokio::test]
async fn large_files_are_stored_in_chunks() {
    let server = TestServer::start(settings());
    let mut alice = connect(&server, Login::named("Alice")).await.unwrap();
    let content: Vec<u8> = (0..17 * 1024 * 1024u32)
        .map(|i| (i.wrapping_mul(7) % 253) as u8)
        .collect();
    let ticket = alice
        .request(Message::RequestUpload {
            channel: 1,
            folder: String::new(),
            name: "gross.bin".into(),
            size: content.len() as u64,
        })
        .await;
    upload(server.address(), &token(ticket), &content)
        .await
        .unwrap();

    let Message::FileList { entries, .. } = alice.request(list(1, "")).await else {
        panic!()
    };
    let back = alice
        .request(Message::RequestDownload {
            file_id: entries[0].id.clone(),
            offset: 16 * 1024 * 1024 - 5,
            stream: false,
        })
        .await;
    assert_eq!(
        download(server.address(), &token(back)).await.unwrap(),
        &content[16 * 1024 * 1024 - 5..]
    );
}

#[tokio::test]
async fn folders_rename_move_and_delete_with_their_content() {
    let server = TestServer::start(settings());
    let mut alice = connect(&server, Login::named("Alice")).await.unwrap();
    let mut bob = connect(&server, Login::named("Bob")).await.unwrap();

    alice
        .ask(Message::CreateFolder {
            channel: 1,
            folder: String::new(),
            name: "A".into(),
        })
        .await
        .unwrap();
    alice
        .ask(Message::CreateFolder {
            channel: 1,
            folder: "A".into(),
            name: "B".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        alice
            .ask(Message::CreateFolder {
                channel: 1,
                folder: String::new(),
                name: "a".into()
            })
            .await,
        Err("Name ist schon vergeben.".into())
    );
    assert_eq!(
        alice
            .ask(Message::CreateFolder {
                channel: 1,
                folder: "Fehlt".into(),
                name: "x".into()
            })
            .await,
        Err("Ordner existiert nicht.".into())
    );
    assert_eq!(
        alice
            .ask(Message::CreateFolder {
                channel: 1,
                folder: String::new(),
                name: "../x".into()
            })
            .await,
        Err("Ungültiger Name.".into())
    );

    let ticket = bob
        .request(Message::RequestUpload {
            channel: 1,
            folder: "A/B".into(),
            name: "x.txt".into(),
            size: 3,
        })
        .await;
    upload(server.address(), &token(ticket), b"abc")
        .await
        .unwrap();

    let folder_a = |m: &Message| match m {
        Message::FileList { entries, .. } => entries[0].id.clone(),
        _ => unreachable!(),
    };
    let a = folder_a(&alice.request(list(1, "")).await);
    assert_eq!(
        alice
            .ask(Message::RenameFile {
                file_id: a.clone(),
                name: "A/B/A".into()
            })
            .await,
        Err("Ein Ordner kann nicht in sich selbst liegen.".into())
    );
    alice
        .ask(Message::RenameFile {
            file_id: a.clone(),
            name: "Neu".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        entries(&alice.request(list(1, "Neu/B")).await),
        [("x.txt".to_owned(), false, 3)]
    );

    // Die eigene Datei darf Bob aendern, Alices Ordner nicht.
    let Message::FileList {
        entries: inside, ..
    } = bob.request(list(1, "Neu/B")).await
    else {
        panic!()
    };
    bob.ask(Message::RenameFile {
        file_id: inside[0].id.clone(),
        name: "/oben.txt".into(),
    })
    .await
    .unwrap();
    assert_eq!(entries(&bob.request(list(1, "")).await).len(), 2);
    assert_eq!(
        bob.ask(Message::DeleteFile { file_id: a.clone() }).await,
        Err("Keine Berechtigung.".into())
    );

    alice.ask(Message::DeleteFile { file_id: a }).await.unwrap();
    assert_eq!(
        entries(&alice.request(list(1, "")).await),
        [("oben.txt".to_owned(), false, 3)]
    );
    assert_eq!(
        alice
            .ask(Message::DeleteFile {
                file_id: "weg".into()
            })
            .await,
        Err("Der Eintrag existiert nicht mehr.".into())
    );
}

#[tokio::test]
async fn files_moved_to_another_channel_follow_with_their_content() {
    let server = TestServer::start(settings());
    let mut alice = connect(&server, Login::named("Alice")).await.unwrap();
    alice
        .ask(Message::CreateChannel {
            name: "Ablage".into(),
            parent: 0,
            permanent: true,
            password: None,
            max_clients: -1,
            topic: None,
        })
        .await
        .unwrap();
    let ticket = alice
        .request(Message::RequestUpload {
            channel: 1,
            folder: String::new(),
            name: "film.mkv".into(),
            size: 6,
        })
        .await;
    upload(server.address(), &token(ticket), b"bilder")
        .await
        .unwrap();
    let Message::FileList { entries, .. } = alice.request(list(1, "")).await else {
        panic!()
    };
    let id = entries[0].id.clone();

    assert_eq!(
        alice
            .ask(Message::MoveFile {
                file_id: id.clone(),
                channel: 9,
                folder: String::new()
            })
            .await,
        Err("Kanal existiert nicht.".into())
    );
    alice
        .ask(Message::MoveFile {
            file_id: id.clone(),
            channel: 2,
            folder: String::new(),
        })
        .await
        .unwrap();
    assert!(entries_of(&alice.request(list(1, "")).await).is_empty());

    // Gleich danach oder nach dem Umzug: der Inhalt kommt immer.
    for _ in 0..20 {
        let ticket = alice
            .request(Message::RequestDownload {
                file_id: id.clone(),
                offset: 0,
                stream: false,
            })
            .await;
        assert_eq!(
            download(server.address(), &token(ticket)).await.unwrap(),
            b"bilder"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

fn entries_of(message: &Message) -> Vec<(String, bool, u64)> {
    entries(message)
}

#[tokio::test]
async fn the_server_wide_limit_and_roles_decide_who_may_upload() {
    let server = TestServer::start(ServerSettings {
        max_file_bytes: 10,
        default_role: Role::Guest,
        ..settings()
    });
    let mut owner = connect(&server, Login::named("Owner")).await.unwrap();
    let mut guest = connect(&server, Login::named("Gast")).await.unwrap();

    assert_eq!(
        guest
            .ask(Message::RequestUpload {
                channel: 1,
                folder: String::new(),
                name: "x".into(),
                size: 1
            })
            .await,
        Err("Keine Berechtigung.".into())
    );
    assert_eq!(
        owner
            .ask(Message::RequestUpload {
                channel: 1,
                folder: String::new(),
                name: "x".into(),
                size: 11
            })
            .await,
        Err("Auf dem Server ist kein Platz mehr.".into())
    );

    // Der Name wird zwischen Anfrage und Ende vergeben: der Upload weicht.
    let first = owner
        .request(Message::RequestUpload {
            channel: 1,
            folder: String::new(),
            name: "doppelt".into(),
            size: 2,
        })
        .await;
    let second = owner
        .request(Message::RequestUpload {
            channel: 1,
            folder: String::new(),
            name: "doppelt".into(),
            size: 2,
        })
        .await;
    upload(server.address(), &token(first), b"ab")
        .await
        .unwrap();
    let refused = upload(server.address(), &token(second), b"cd").await;
    assert_eq!(
        refused.err().as_deref(),
        Some("Der Ort gibt es nicht mehr oder der Name ist inzwischen vergeben.")
    );
}

#[tokio::test]
async fn chat_images_arrive_as_messages_and_download_from_the_archive() {
    let server = TestServer::start(settings());
    let mut alice = connect(&server, Login::named("Alice")).await.unwrap();
    let mut bob = connect(&server, Login::named("Bob")).await.unwrap();

    let image = |size| Message::RequestChatImage {
        scope: ChatScope::Channel,
        target: 0,
        size,
        width: 1,
        height: 1,
    };
    assert!(
        alice
            .ask(image(17 * 1024 * 1024))
            .await
            .unwrap_err()
            .contains("höchstens 16 MB")
    );
    assert_eq!(
        alice
            .ask(Message::RequestChatImage {
                scope: ChatScope::Private,
                target: 77,
                size: 5,
                width: 1,
                height: 1
            })
            .await,
        Err("Der Empfänger ist nicht mehr online.".into())
    );

    let fake = alice.request(image(4)).await;
    assert!(
        upload(server.address(), &token(fake), b"PK\x03\x04")
            .await
            .unwrap_err()
            .contains("kein Bild")
    );

    let ticket = alice.request(image(PNG.len() as u64)).await;
    upload(server.address(), &token(ticket), PNG).await.unwrap();
    let Message::ChatEvent {
        archive, message, ..
    } = bob.event(|m| matches!(m, Message::ChatEvent { .. })).await
    else {
        unreachable!()
    };
    let shown = message.image.expect("Bild im Verlauf");
    assert_eq!(
        (shown.width, shown.height, shown.size, message.text.as_str()),
        (1, 1, PNG.len() as u64, "")
    );

    let back = bob
        .request(Message::RequestImageDownload {
            archive: archive.clone(),
            image_id: shown.id.clone(),
        })
        .await;
    assert_eq!(download(server.address(), &token(back)).await.unwrap(), PNG);
    assert_eq!(
        bob.ask(Message::RequestImageDownload {
            archive,
            image_id: "gibtsnicht".into()
        })
        .await,
        Err("Das Bild gibt es in diesem Verlauf nicht.".into())
    );

    let everyone = ArchiveRef {
        scope: ChatScope::Server,
        channel: 0,
        peer: String::new(),
    };
    let Message::ChatHistory { messages, .. } = bob
        .request(Message::SyncChat {
            archive: everyone,
            after_seq: 0,
        })
        .await
    else {
        panic!()
    };
    assert!(messages.is_empty());
}
