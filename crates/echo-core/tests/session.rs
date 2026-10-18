//! Die Session gegen einen echten Server im Prozess.

#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::{TestClient, TestServer, chat, event, settings, until};
use echo_core::{ChatArchive, ChatScope, ConnectOptions, ConnectionState, EventKind};
use echo_protocol::{Role, ShapeDto};
use tokio::sync::mpsc;

#[tokio::test]
async fn connects_remembers_the_host_and_sees_others() {
    let server = TestServer::start(settings());
    let alice = TestClient::new();
    let changes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&changes);
    alice.session.on_change(move || {
        counter.fetch_add(1, Ordering::Relaxed);
    });
    alice
        .session
        .connect(ConnectOptions::new(&server.address(), "Alice"))
        .await
        .unwrap();
    assert_eq!(alice.session.state(), ConnectionState::Connected);
    assert!(changes.load(Ordering::Relaxed) > 0);
    assert_eq!(alice.session.address(), server.address());
    assert_eq!(
        alice.session.host_fingerprint(),
        server.server.host_fingerprint()
    );
    event(&alice.session, |k| match k {
        EventKind::Info(text) if text.contains("Host-Schlüssel gemerkt") => Some(()),
        _ => None,
    })
    .await;
    let snapshot = alice.session.snapshot();
    assert_eq!(snapshot.server_name, "Test");
    assert_eq!(snapshot.own_role, Some(Role::Owner));
    assert!(snapshot.own_channel().is_some());

    let bob = TestClient::connect(&server, "Bob").await;
    let joined = event(&alice.session, |k| match k {
        EventKind::ClientJoined { name, channel, .. } => Some((name.clone(), channel.clone())),
        _ => None,
    })
    .await;
    assert_eq!(joined.0, "Bob");
    assert_eq!(joined.1, snapshot.own_channel().unwrap().display_name());
    until(&alice.session, |s| s.clients.len() == 2).await;

    bob.session.disconnect().await;
    assert_eq!(bob.session.state(), ConnectionState::Disconnected);
    let reason = event(&alice.session, |k| match k {
        EventKind::ClientLeft { name, reason, .. } if name == "Bob" => Some(reason.clone()),
        _ => None,
    })
    .await;
    assert_eq!(reason, "hat den Server verlassen");
    event(&bob.session, |k| match k {
        EventKind::Disconnected {
            was_error: false, ..
        } => Some(()),
        _ => None,
    })
    .await;

    // Ein zweites Mal ist derselbe Server bekannt: keine neue Meldung.
    let again = alice.restart();
    again
        .connect(ConnectOptions::new(&server.address(), "Alice"))
        .await
        .unwrap();
    assert!(
        std::iter::from_fn(|| again.next_event()).all(|e| match e.kind {
            EventKind::Info(text) => !text.contains("gemerkt"),
            _ => true,
        })
    );
    alice.session.disconnect().await;
    again.disconnect().await;
    server.server.stop().await;
}

#[tokio::test]
async fn a_changed_host_key_is_refused_until_trusted() {
    let server = TestServer::start(settings());
    let alice = TestClient::new();
    let address = server.address();
    let options = ConnectOptions::new(&address, "Alice");
    alice.session.connect(options.clone()).await.unwrap();
    alice.session.disconnect().await;

    alice
        .session
        .trust_host_key(&address, "SHA256:anders")
        .unwrap();
    let error = alice.session.connect(options.clone()).await.unwrap_err();
    assert!(error.contains("hat sich geändert"), "{error}");
    let (host, known, offered) = event(&alice.session, |k| match k {
        EventKind::HostKeyChanged {
            host,
            known,
            offered,
            ..
        } => Some((host.clone(), known.clone(), offered.clone())),
        _ => None,
    })
    .await;
    assert_eq!(host, address);
    assert_eq!(known, "SHA256:anders");
    assert_eq!(offered, server.server.host_fingerprint());
    assert_eq!(alice.session.state(), ConnectionState::Disconnected);

    alice.session.trust_host_key(&host, &offered).unwrap();
    alice.session.connect(options).await.unwrap();
    alice.session.disconnect().await;
    server.server.stop().await;
}

#[tokio::test]
async fn passwords_versions_and_missing_hosts_give_reasons() {
    let server = TestServer::start(echo_server::ServerSettings {
        password: Some("geheim".into()),
        ..settings()
    });
    let alice = TestClient::new();
    let mut options = ConnectOptions::new(&server.address(), "Alice");
    let error = alice.session.connect(options.clone()).await.unwrap_err();
    assert_eq!(error, "Dieser Server verlangt ein Passwort.");

    options.server_password = Some("falsch".into());
    assert!(alice.session.connect(options.clone()).await.is_err());

    options.server_password = Some("geheim".into());
    alice.session.connect(options).await.unwrap();
    alice.session.disconnect().await;
    // Ein zweites Trennen tut nichts.
    alice.session.disconnect().await;

    let error = alice
        .session
        .connect(ConnectOptions::new("nicht-da.invalid:1", "Alice"))
        .await
        .unwrap_err();
    assert!(error.contains("nicht zu finden"), "{error}");
    server.server.stop().await;
}

#[tokio::test]
async fn chat_reaches_everyone_with_numbers_and_names() {
    let server = TestServer::start(settings());
    let alice = TestClient::connect(&server, "Alice").await;
    let bob = TestClient::connect(&server, "Bob").await;
    until(&alice.session, |s| s.clients.len() == 2).await;

    alice
        .session
        .send_channel_message("Hallo Kanal")
        .await
        .unwrap();
    let mine = chat(&alice.session, "Hallo Kanal").await;
    let theirs = chat(&bob.session, "Hallo Kanal").await;
    assert!(mine.own && !theirs.own);
    assert_eq!(theirs.from, "Alice");
    assert_eq!(mine.archive.scope, ChatScope::Channel);
    assert_eq!(mine.seq, theirs.seq);

    bob.session
        .send_server_message("Hallo Server")
        .await
        .unwrap();
    assert_eq!(
        chat(&alice.session, "Hallo Server").await.archive,
        ChatArchive::server()
    );

    let bob_id = bob.session.snapshot().own_client_id;
    alice
        .session
        .send_private_message(bob_id, "Psst")
        .await
        .unwrap();
    let private = chat(&bob.session, "Psst").await;
    assert_eq!(private.archive.scope, ChatScope::Private);
    assert_eq!(private.peer_name, "Alice");
    let echo = chat(&alice.session, "Psst").await;
    assert_eq!(echo.peer_name, "Bob");

    alice.session.poke(bob_id, "Hey").await.unwrap();
    let poke = event(&bob.session, |k| match k {
        EventKind::Poke { from, text, .. } => Some((from.clone(), text.clone())),
        _ => None,
    })
    .await;
    assert_eq!(poke, ("Alice".into(), "Hey".into()));

    let error = alice.session.join_channel(9999, None).await.unwrap_err();
    assert!(!error.is_empty());
    event(&alice.session, |k| match k {
        EventKind::Error(text) if text.starts_with("Kanal wechseln: ") => Some(()),
        _ => None,
    })
    .await;
    server.server.stop().await;
}

#[tokio::test]
async fn the_mirror_keeps_history_across_restarts() {
    let server = TestServer::start(settings());
    let alice = TestClient::connect(&server, "Alice").await;
    for i in 0..3 {
        alice
            .session
            .send_channel_message(&format!("n{i}"))
            .await
            .unwrap();
        chat(&alice.session, &format!("n{i}")).await;
    }
    alice.session.disconnect().await;
    assert!(alice.mirror().exists());

    // Waehrend Alice weg ist, schreibt Bob weiter.
    let bob = TestClient::connect(&server, "Bob").await;
    bob.session.send_channel_message("verpasst").await.unwrap();
    chat(&bob.session, "verpasst").await;

    let again = alice.restart();
    again
        .connect(ConnectOptions::new(&server.address(), "Alice"))
        .await
        .unwrap();
    let missed = chat(&again, "verpasst").await;
    assert_eq!(missed.seq, 4);
    let history = again.history();
    let texts: Vec<_> = history.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(texts, ["n0", "n1", "n2", "verpasst"]);

    let channel = ChatArchive::channel(again.snapshot().own_channel().unwrap().id);
    again.load_older(channel.clone(), 3).await;
    let older = event(&again, |k| match k {
        EventKind::OlderHistory { lines, .. } => {
            Some(lines.iter().map(|l| l.seq).collect::<Vec<_>>())
        }
        _ => None,
    })
    .await;
    assert_eq!(older, [1, 2]);

    // Ohne Spiegel kommt die Seite vom Server.
    let fresh = TestClient::connect(&server, "Carol").await;
    fresh.session.load_older(channel, 4).await;
    let older = event(&fresh.session, |k| match k {
        EventKind::OlderHistory { lines, .. } => Some(lines.len()),
        _ => None,
    })
    .await;
    assert_eq!(older, 3);
    server.server.stop().await;
}

#[tokio::test]
async fn private_conversations_are_fetched_after_reconnect() {
    let server = TestServer::start(settings());
    let alice = TestClient::connect(&server, "Alice").await;
    let bob = TestClient::connect(&server, "Bob").await;
    until(&bob.session, |s| s.clients.len() == 2).await;
    let alice_id = alice.session.snapshot().own_client_id;
    bob.session
        .send_private_message(alice_id, "Bist du da?")
        .await
        .unwrap();
    chat(&alice.session, "Bist du da?").await;

    // Bob kommt mit leerem Spiegel wieder: das Gespraech wird nachgeholt.
    bob.session.disconnect().await;
    std::fs::remove_dir_all(bob.mirror()).unwrap();
    let again = bob.restart();
    again
        .connect(ConnectOptions::new(&server.address(), "Bob"))
        .await
        .unwrap();
    let line = chat(&again, "Bist du da?").await;
    assert!(line.own);
    assert_eq!(line.peer_name, "Alice");
    server.server.stop().await;
}

#[tokio::test]
async fn voice_goes_to_the_others_in_the_channel() {
    let server = TestServer::start(settings());
    let alice = TestClient::connect(&server, "Alice").await;
    let bob = TestClient::connect(&server, "Bob").await;
    until(&alice.session, |s| s.clients.len() == 2).await;
    let (sink, mut heard) = mpsc::unbounded_channel();
    bob.session.on_voice(move |voice| {
        let _ = sink.send(voice);
    });

    let alice_id = alice.session.snapshot().own_client_id;
    alice.session.send_voice(&[1, 2, 3]);
    alice.session.send_voice(&[]);
    let first = tokio::time::timeout(common::TIMEOUT, heard.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.sender, alice_id);
    assert_eq!(first.opus, [1, 2, 3]);
    let end = tokio::time::timeout(common::TIMEOUT, heard.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(end.opus.is_empty());
    assert_eq!(end.sequence, first.sequence.wrapping_add(1));

    // Ohne Verbindung geht nichts raus, und nichts bricht.
    bob.session.disconnect().await;
    bob.session.send_voice(&[1]);
    server.server.stop().await;
}

#[tokio::test]
async fn channels_are_created_edited_joined_and_deleted() {
    let server = TestServer::start(settings());
    let alice = TestClient::connect(&server, "Alice").await;
    let bob = TestClient::connect(&server, "Bob").await;
    until(&alice.session, |s| s.clients.len() == 2).await;

    alice
        .session
        .create_channel("Werkstatt", 0, true)
        .await
        .unwrap();
    until(&alice.session, |s| {
        s.channels.iter().any(|c| c.name == "Werkstatt")
    })
    .await;
    let id = alice
        .session
        .snapshot()
        .channels
        .iter()
        .find(|c| c.name == "Werkstatt")
        .unwrap()
        .id;
    alice
        .session
        .edit_channel(id, "Werkbank", Some("Thema".into()), Some("pw".into()), 0)
        .await
        .unwrap();
    until(&bob.session, |s| {
        s.channel(id)
            .is_some_and(|c| c.name == "Werkbank" && c.has_password)
    })
    .await;

    assert!(bob.session.join_channel(id, None).await.is_err());
    bob.session
        .join_channel(id, Some("pw".into()))
        .await
        .unwrap();
    let moved = event(&bob.session, |k| match k {
        EventKind::ClientMoved {
            is_self: true,
            to,
            invoker,
            ..
        } => Some((to.clone(), invoker.clone())),
        _ => None,
    })
    .await;
    assert_eq!(moved, ("Werkbank".into(), None));
    until(&bob.session, |s| {
        s.own_channel().is_some_and(|c| c.id == id)
    })
    .await;

    let bob_id = bob.session.snapshot().own_client_id;
    let lobby = alice.session.snapshot().own_channel().unwrap().id;
    alice.session.move_client(bob_id, lobby).await.unwrap();
    let invoker = event(&bob.session, |k| match k {
        EventKind::ClientMoved {
            is_self: true,
            invoker,
            ..
        } => Some(invoker.clone()),
        _ => None,
    })
    .await;
    assert_eq!(invoker.as_deref(), Some("Alice"));

    alice.session.delete_channel(id).await.unwrap();
    until(&bob.session, |s| s.channel(id).is_none()).await;
    assert!(bob.session.delete_channel(lobby).await.is_err());
    server.server.stop().await;
}

#[tokio::test]
async fn state_roles_and_moderation_reach_everyone() {
    let server = TestServer::start(settings());
    let alice = TestClient::connect(&server, "Alice").await;
    let bob = TestClient::connect(&server, "Bob").await;
    until(&alice.session, |s| s.clients.len() == 2).await;
    let bob_id = bob.session.snapshot().own_client_id;

    bob.session.set_input_muted(true).await.unwrap();
    bob.session.set_output_muted(true).await.unwrap();
    bob.session.set_away(true, "Kaffee").await.unwrap();
    bob.session.change_nickname("Robert").await.unwrap();
    until(&alice.session, |s| {
        s.client(bob_id).is_some_and(|c| {
            c.input_muted
                && c.output_muted
                && c.away_message.as_deref() == Some("Kaffee")
                && c.name == "Robert"
        })
    })
    .await;

    let fingerprint = bob.identity.fingerprint().to_owned();
    alice
        .session
        .set_role(&fingerprint, Role::Admin)
        .await
        .unwrap();
    until(&bob.session, |s| s.own_role == Some(Role::Admin)).await;
    assert!(bob.session.approvals().await.unwrap().is_empty());
    alice
        .session
        .set_role(&fingerprint, Role::Member)
        .await
        .unwrap();

    alice.session.kick(bob_id, false, "raus").await.unwrap();
    event(&bob.session, |k| match k {
        EventKind::Info(text) if text.contains("aus dem Kanal geworfen: raus") => Some(()),
        _ => None,
    })
    .await;

    alice.session.kick(bob_id, true, "ganz raus").await.unwrap();
    let reason = event(&bob.session, |k| match k {
        EventKind::Disconnected { reason, was_error } => Some((reason.clone(), *was_error)),
        _ => None,
    })
    .await;
    assert_eq!(
        reason,
        (
            "Alice hat dich vom Server geworfen: ganz raus".into(),
            false
        )
    );
    let left = event(&alice.session, |k| match k {
        EventKind::ClientLeft { reason, .. } => Some(reason.clone()),
        _ => None,
    })
    .await;
    assert!(left.starts_with("wurde vom Server geworfen"), "{left}");

    let carol = TestClient::connect(&server, "Carol").await;
    until(&alice.session, |s| s.clients.len() == 2).await;
    let carol_id = carol.session.snapshot().own_client_id;
    alice.session.ban(carol_id, "weg", 60).await.unwrap();
    let left = event(&alice.session, |k| match k {
        EventKind::ClientLeft { reason, .. } => Some(reason.clone()),
        _ => None,
    })
    .await;
    assert!(left.starts_with("wurde gebannt"), "{left}");
    assert!(
        carol
            .session
            .connect(ConnectOptions::new(&server.address(), "Carol"))
            .await
            .is_err()
    );
    server.server.stop().await;
}

#[tokio::test]
async fn approvals_are_listed_and_decided() {
    let server = TestServer::start(echo_server::ServerSettings {
        require_approval: true,
        ..settings()
    });
    let alice = TestClient::connect(&server, "Alice").await;
    let bob = TestClient::new();
    let options = ConnectOptions::new(&server.address(), "Bob");
    assert!(bob.session.connect(options.clone()).await.is_err());
    let pending = event(&alice.session, |k| match k {
        EventKind::ApprovalsChanged { pending } => Some(*pending),
        _ => None,
    })
    .await;
    assert_eq!(pending, 1);
    let entries = alice.session.approvals().await.unwrap();
    assert_eq!(entries[0].name, "Bob");
    alice
        .session
        .decide_approval(&entries[0].fingerprint, true)
        .await
        .unwrap();
    bob.session.connect(options).await.unwrap();
    server.server.stop().await;
}

#[tokio::test]
async fn the_file_area_lists_and_changes() {
    let server = TestServer::start(settings());
    let alice = TestClient::connect(&server, "Alice").await;
    let channel = alice.session.snapshot().own_channel().unwrap().id;

    alice
        .session
        .create_folder(channel, "", "Bilder")
        .await
        .unwrap();
    event(&alice.session, |k| match k {
        EventKind::FilesChanged { channel: c } if *c == channel => Some(()),
        _ => None,
    })
    .await;
    let listing = alice.session.list_files(channel, "").await.unwrap();
    assert_eq!(listing.entries.len(), 1);
    let folder = listing.entries[0].id.clone();
    assert!(listing.entries[0].is_folder);

    alice.session.rename_file(&folder, "Fotos").await.unwrap();
    alice
        .session
        .create_folder(channel, "", "Archiv")
        .await
        .unwrap();
    alice
        .session
        .move_file(&folder, channel, "Archiv")
        .await
        .unwrap();
    assert_eq!(
        alice
            .session
            .list_files(channel, "Archiv")
            .await
            .unwrap()
            .entries[0]
            .name,
        "Fotos"
    );
    alice.session.delete_file(&folder).await.unwrap();
    assert!(
        alice
            .session
            .list_files(channel, "Archiv")
            .await
            .unwrap()
            .entries
            .is_empty()
    );
    assert!(alice.session.list_files(9999, "").await.is_err());
    assert!(alice.session.delete_file("gibtsnicht").await.is_err());
    server.server.stop().await;
}

#[tokio::test]
async fn boards_open_edit_and_point() {
    let server = TestServer::start(settings());
    let alice = TestClient::connect(&server, "Alice").await;
    let bob = TestClient::connect(&server, "Bob").await;
    until(&alice.session, |s| s.clients.len() == 2).await;
    let channel = alice.session.snapshot().own_channel().unwrap().id;

    alice.session.create_board(channel, "Plan").await.unwrap();
    event(&bob.session, |k| match k {
        EventKind::BoardsChanged { .. } => Some(()),
        _ => None,
    })
    .await;
    let boards = bob.session.list_boards(channel).await.unwrap();
    let board = boards[0].id.clone();
    alice
        .session
        .rename_board(&board, "Großer Plan")
        .await
        .unwrap();

    for session in [&alice.session, &bob.session] {
        session.open_board(&board).await.unwrap();
        let name = event(session, |k| match k {
            EventKind::BoardOpened { name, .. } => Some(name.clone()),
            _ => None,
        })
        .await;
        assert_eq!(name, "Großer Plan");
    }
    alice.session.join_board(&board, true).await.unwrap();
    event(&bob.session, |k| match k {
        EventKind::BoardPresence { participants, .. } if !participants.is_empty() => Some(()),
        _ => None,
    })
    .await;

    let shape = ShapeDto {
        id: "s1".into(),
        json: Some("{}".into()),
    };
    alice
        .session
        .edit_board(&board, vec![shape.clone()])
        .await
        .unwrap();
    let own = event(&alice.session, |k| match k {
        EventKind::BoardEdited { own, changes, .. } => Some((*own, changes.clone())),
        _ => None,
    })
    .await;
    assert_eq!(own, (true, vec![shape.clone()]));
    let theirs = event(&bob.session, |k| match k {
        EventKind::BoardEdited { own, .. } => Some(*own),
        _ => None,
    })
    .await;
    assert!(!theirs);
    // Wer nur zusieht, darf nicht zeichnen.
    assert!(bob.session.edit_board(&board, vec![shape]).await.is_err());

    alice.session.board_pointer(&board, 1.0, 2.0, true);
    let pointer = event(&bob.session, |k| match k {
        EventKind::BoardPointer { x, y, laser, .. } => Some((*x, *y, *laser)),
        _ => None,
    })
    .await;
    assert_eq!(pointer, (1.0, 2.0, true));

    alice.session.join_board(&board, false).await.unwrap();
    alice.session.close_board(&board);
    bob.session.close_board(&board);
    alice.session.delete_board(&board).await.unwrap();
    assert!(alice.session.open_board(&board).await.is_err());
    assert!(bob.session.list_boards(9999).await.is_err());
    server.server.stop().await;
}

#[tokio::test]
async fn commands_without_a_connection_fail_quietly() {
    let alice = TestClient::new();
    assert_eq!(alice.session.round_trip(), None);
    assert_eq!(
        alice.session.join_channel(1, None).await.unwrap_err(),
        "Nicht verbunden."
    );
    assert!(alice.session.list_files(1, "").await.is_err());
    alice.session.board_pointer("b", 0.0, 0.0, false);
    alice.session.load_older(ChatArchive::server(), 10).await;
    assert!(alice.session.history().is_empty());
    assert!(format!("{:?}", alice.session).contains("Disconnected"));
}

#[tokio::test]
async fn the_server_going_away_is_reported() {
    let server = TestServer::start(settings());
    let alice = TestClient::connect(&server, "Alice").await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(alice.session.round_trip().is_some());
    server.server.stop().await;
    event(&alice.session, |k| match k {
        EventKind::Disconnected { .. } => Some(()),
        _ => None,
    })
    .await;
    assert_eq!(alice.session.state(), ConnectionState::Disconnected);
}
