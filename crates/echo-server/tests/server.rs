//! Der Server im Prozess mit echten QUIC-Clients.

// Szenarien lesen sich am besten am Stueck.
#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

mod common;

use std::time::Duration;

use common::{Login, TestServer, connect, connect_to, open, read, settings};
use echo_protocol::{
    ArchiveRef, ChatScope, Envelope, FrameKind, Identity, LeaveReason, Message, Role, VoiceFrame,
    Wire, write_message,
};
use echo_server::{EchoServer, ServerPaths, ServerSettings};

const QUIET: Duration = Duration::from_millis(300);

fn chat_text(message: &Message) -> Option<&str> {
    match message {
        Message::ChatEvent { message, .. } => Some(&message.text),
        _ => None,
    }
}

#[tokio::test]
async fn first_key_owns_the_server_and_others_see_who_joins() {
    let server = TestServer::start(settings());
    let mut alice = connect(&server, Login::named("Alice")).await.unwrap();
    assert_eq!(alice.role(), Role::Owner);
    assert_eq!(alice.channel_names(), ["Lobby"]);

    let bob = connect(&server, Login::named("Alice")).await.unwrap();
    assert_eq!(bob.role(), Role::Member);
    let Message::ClientJoined { client } = alice
        .event(|m| matches!(m, Message::ClientJoined { .. }))
        .await
    else {
        unreachable!()
    };
    // Zwei Clients mit demselben Namen waeren im Baum nicht zu unterscheiden.
    assert_eq!(client.name, "Alice1");
    assert_eq!(server.server.client_count(), 2);

    assert!(matches!(alice.request(Message::Ping).await, Message::Pong));
    assert_eq!(
        alice
            .ask(Message::Ban {
                client_id: 99,
                reason: String::new(),
                duration_seconds: 0
            })
            .await,
        Err("Client ist nicht online.".into())
    );
    assert_eq!(
        alice.ask(Message::ListBoards { channel: 1 }).await,
        Err("Unbekannte Anfrage.".into())
    );

    drop(bob);
    server.server.stop().await;
}

#[tokio::test]
async fn refuses_other_versions_with_a_readable_reason() {
    let server = TestServer::start(settings());
    let (_endpoint, _connection, _send, mut recv) = open(server.address(), 1).await;
    let Message::Reply {
        ok: false,
        error: Some(error),
    } = read(&mut recv).await.message
    else {
        panic!("Ablehnung erwartet");
    };
    assert!(error.contains("Client ist älter"), "{error}");
}

#[tokio::test]
async fn refuses_signatures_for_another_name() {
    let server = TestServer::start(settings());
    let (_endpoint, _connection, mut send, mut recv) = open(server.address(), Wire::VERSION).await;
    let Message::Challenge { nonce, .. } = read(&mut recv).await.message else {
        panic!("Challenge erwartet");
    };
    let identity = Identity::generate().unwrap();
    let signed_for_other_name = identity.sign(&echo_protocol::hello_payload(
        &nonce,
        &server.host(),
        "Mallory",
    ));
    let hello = Message::Hello {
        nickname: "Alice".into(),
        public_key: identity.public_key().into(),
        signature: signed_for_other_name,
        password_proof: None,
        default_channel: 0,
        default_channel_password: None,
        input_muted: false,
        output_muted: false,
    };
    write_message(
        &mut send,
        &Envelope {
            request_id: 2,
            message: hello,
        },
    )
    .await
    .unwrap();
    let Message::Reply { error, .. } = read(&mut recv).await.message else {
        panic!("Ablehnung erwartet");
    };
    assert_eq!(error.as_deref(), Some("Signatur ungültig."));
}

#[tokio::test]
async fn the_server_password_is_proven_not_sent() {
    let server = TestServer::start(ServerSettings {
        password: Some("geheim".into()),
        ..settings()
    });
    let refused = connect(&server, Login::named("Alice")).await.err();
    assert_eq!(refused.as_deref(), Some("Falsches Serverpasswort."));
    let wrong = connect(
        &server,
        Login {
            password: Some("falsch"),
            ..Login::named("Alice")
        },
    )
    .await;
    assert!(wrong.is_err());
    assert!(
        connect(
            &server,
            Login {
                password: Some("geheim"),
                ..Login::named("Alice")
            }
        )
        .await
        .is_ok()
    );
}

#[tokio::test]
async fn channel_chat_gets_numbers_and_is_synced_later() {
    let server = TestServer::start(settings());
    let mut alice = connect(&server, Login::named("Alice")).await.unwrap();
    let mut bob = connect(&server, Login::named("Bob")).await.unwrap();

    alice
        .tell(Message::SendChat {
            scope: ChatScope::Channel,
            target: 0,
            text: "  hallo  ".into(),
        })
        .await;
    let heard = bob.event(|m| chat_text(m).is_some()).await;
    let Message::ChatEvent {
        archive,
        from_id,
        message,
    } = heard
    else {
        unreachable!()
    };
    assert_eq!(
        (archive.scope, archive.channel, from_id),
        (ChatScope::Channel, 1, alice.id)
    );
    assert_eq!(
        (
            message.seq,
            message.text.as_str(),
            message.author_name.as_str()
        ),
        (1, "hallo", "Alice")
    );
    assert_eq!(
        chat_text(&alice.event(|m| chat_text(m).is_some()).await),
        Some("hallo")
    );

    alice
        .tell(Message::SendChat {
            scope: ChatScope::Server,
            target: 0,
            text: "an alle".into(),
        })
        .await;
    assert_eq!(
        chat_text(&bob.event(|m| chat_text(m) == Some("an alle")).await),
        Some("an alle")
    );

    let room = ArchiveRef {
        scope: ChatScope::Channel,
        channel: 1,
        peer: String::new(),
    };
    let Message::ChatHistory { messages, more, .. } = bob
        .request(Message::SyncChat {
            archive: room.clone(),
            after_seq: 0,
        })
        .await
    else {
        panic!("Verlauf erwartet")
    };
    assert_eq!(messages.len(), 1);
    assert!(!more);
    let Message::ChatHistory { messages, .. } = bob
        .request(Message::LoadHistory {
            archive: room,
            before_seq: 2,
        })
        .await
    else {
        panic!("Verlauf erwartet")
    };
    assert_eq!(messages[0].text, "hallo");

    let elsewhere = ArchiveRef {
        scope: ChatScope::Channel,
        channel: 99,
        peer: String::new(),
    };
    let refused = bob
        .request(Message::SyncChat {
            archive: elsewhere,
            after_seq: 0,
        })
        .await;
    assert!(matches!(refused, Message::Reply { ok: false, .. }));
}

#[tokio::test]
async fn private_chats_reach_both_and_are_offered_after_reconnect() {
    let server = TestServer::start(settings());
    let mut alice = connect(&server, Login::named("Alice")).await.unwrap();
    let bob_key = Identity::generate().unwrap();
    let bob_pem = bob_key.to_pem().unwrap();
    let mut bob = connect(
        &server,
        Login {
            identity: Some(bob_key),
            ..Login::named("Bob")
        },
    )
    .await
    .unwrap();

    alice
        .tell(Message::SendChat {
            scope: ChatScope::Private,
            target: bob.id,
            text: "psst".into(),
        })
        .await;
    let Message::ChatEvent { archive, .. } = bob.event(|m| chat_text(m).is_some()).await else {
        unreachable!()
    };
    assert_eq!(archive.peer, alice.identity.fingerprint());
    let Message::ChatEvent { archive, .. } = alice.event(|m| chat_text(m).is_some()).await else {
        unreachable!()
    };
    assert_eq!(archive.peer, bob.identity.fingerprint());

    // An jemanden, der nicht online ist: der Absender erfaehrt es.
    alice
        .tell(Message::SendChat {
            scope: ChatScope::Private,
            target: 77,
            text: "?".into(),
        })
        .await;
    let gone = alice
        .event(|m| matches!(m, Message::ClientLeft { client_id: 77, .. }))
        .await;
    assert!(matches!(gone, Message::ClientLeft { .. }));

    drop(bob);
    let mut again = connect(
        &server,
        Login {
            identity: Some(Identity::from_pem(&bob_pem).unwrap()),
            ..Login::named("Bob")
        },
    )
    .await
    .unwrap();
    let Message::DmConversations { conversations } = again
        .event(|m| matches!(m, Message::DmConversations { .. }))
        .await
    else {
        unreachable!()
    };
    assert_eq!(conversations.len(), 1);
    assert_eq!(
        (
            conversations[0].peer_name.as_str(),
            conversations[0].last_seq
        ),
        ("Alice", 1)
    );
}

#[tokio::test]
async fn voice_goes_to_the_others_in_the_channel_as_datagrams() {
    let server = TestServer::start(settings());
    let mut alice = connect(&server, Login::named("Alice")).await.unwrap();
    let bob = connect(&server, Login::named("Bob")).await.unwrap();
    alice
        .event(|m| matches!(m, Message::ClientJoined { .. }))
        .await;

    alice.send_voice(7, b"opus");
    let heard = bob.voice(common::TIMEOUT).await.expect("Bob hoert nichts");
    let frame = VoiceFrame::parse(&heard).unwrap();
    assert_eq!(
        (frame.kind, frame.sender, frame.sequence, frame.opus),
        (FrameKind::Voice, alice.id, 7, &b"opus"[..])
    );
    // Die eigene Stimme waere ein Echo.
    assert!(alice.voice(QUIET).await.is_none());

    // Als eigener Stream, wenn Datagramme nicht gehen.
    let mut stream = alice.connection.open_uni().await.unwrap();
    let frame = VoiceFrame {
        kind: FrameKind::Voice,
        sender: 0,
        sequence: 8,
        timestamp_ms: 0,
        opus: b"strom",
    };
    stream.write_all(&frame.to_bytes()).await.unwrap();
    stream.finish().unwrap();
    let heard = bob
        .voice(common::TIMEOUT)
        .await
        .expect("Bob hoert den Stream nicht");
    assert_eq!(VoiceFrame::parse(&heard).unwrap().opus, b"strom");

    // Stumm geschaltet geht nichts weiter.
    alice
        .tell(Message::SetState {
            input_muted: Some(true),
            output_muted: None,
            away: None,
            away_message: None,
            nickname: None,
        })
        .await;
    alice.request(Message::Ping).await;
    alice.send_voice(9, b"leise");
    assert!(bob.voice(QUIET).await.is_none());
}

#[tokio::test]
async fn temporary_channels_vanish_with_the_last_member() {
    let server = TestServer::start(settings());
    let mut alice = connect(&server, Login::named("Alice")).await.unwrap();
    let mut bob = connect(&server, Login::named("Bob")).await.unwrap();

    let request = |permanent| Message::CreateChannel {
        name: " Raum ".into(),
        parent: 0,
        permanent,
        password: Some("pw".into()),
        max_clients: -1,
        topic: None,
    };
    assert_eq!(
        bob.ask(request(true)).await,
        Err("Dauerhafte Kanäle darf nur ein Admin anlegen.".into())
    );
    bob.ask(request(false)).await.unwrap();
    let Message::ChannelAdded { channel } = alice
        .event(|m| matches!(m, Message::ChannelAdded { .. }))
        .await
    else {
        unreachable!()
    };
    assert_eq!(
        (
            channel.name.as_str(),
            channel.has_password,
            channel.permanent
        ),
        ("Raum", true, false)
    );

    assert_eq!(
        bob.ask(Message::JoinChannel {
            channel_id: channel.id,
            password: None
        })
        .await,
        Err("Falsches Kanalpasswort.".into())
    );
    bob.ask(Message::JoinChannel {
        channel_id: channel.id,
        password: Some("pw".into()),
    })
    .await
    .unwrap();
    bob.ask(Message::JoinChannel {
        channel_id: 1,
        password: None,
    })
    .await
    .unwrap();
    let removed = alice
        .event(|m| matches!(m, Message::ChannelRemoved { .. }))
        .await;
    assert!(matches!(removed, Message::ChannelRemoved { channel_id } if channel_id == channel.id));
}

#[tokio::test]
async fn owners_edit_permanent_channels_and_the_default_stays() {
    let server = TestServer::start(settings());
    let mut alice = connect(&server, Login::named("Alice")).await.unwrap();
    alice
        .ask(Message::CreateChannel {
            name: "Ablage".into(),
            parent: 0,
            permanent: true,
            password: None,
            max_clients: 1,
            topic: None,
        })
        .await
        .unwrap();
    alice
        .ask(Message::EditChannel {
            channel_id: 2,
            name: "Archiv".into(),
            topic: Some("  alt  ".into()),
            change_password: true,
            password: Some("x".into()),
            needed_talk_power: -5,
        })
        .await
        .unwrap();
    let Message::ChannelChanged { channel } = alice
        .event(|m| matches!(m, Message::ChannelChanged { .. }))
        .await
    else {
        unreachable!()
    };
    assert_eq!(
        (
            channel.name.as_str(),
            channel.topic.as_deref(),
            channel.has_password,
            channel.needed_talk_power
        ),
        ("Archiv", Some("alt"), true, 0)
    );
    assert_eq!(
        alice.ask(Message::DeleteChannel { channel_id: 1 }).await,
        Err("Der Standardkanal bleibt.".into())
    );
    assert_eq!(
        alice
            .ask(Message::EditChannel {
                channel_id: 9,
                name: "x".into(),
                topic: None,
                change_password: false,
                password: None,
                needed_talk_power: 0
            })
            .await,
        Err("Kanal existiert nicht.".into())
    );

    // Ein Neustart behaelt den dauerhaften Kanal.
    let data = server.dir.path().to_owned();
    let TestServer {
        server: running,
        dir,
    } = server;
    drop(alice);
    running.stop().await;
    let restarted = TestServer::start_in(&data, None);
    let restarted = TestServer {
        server: restarted,
        dir,
    };
    let carol = connect(&restarted, Login::named("Carol")).await.unwrap();
    assert_eq!(carol.channel_names(), ["Lobby", "Archiv"]);

    let mut alice_again = connect(&restarted, Login::named("Admin")).await.unwrap();
    assert_eq!(
        alice_again
            .ask(Message::DeleteChannel { channel_id: 2 })
            .await,
        Err("Keine Berechtigung.".into())
    );
}

#[tokio::test]
async fn moderators_move_kick_and_ban_lower_ranks() {
    let server = TestServer::start(settings());
    let mut alice = connect(&server, Login::named("Alice")).await.unwrap();
    let bob_key = Identity::generate().unwrap();
    let bob_pem = bob_key.to_pem().unwrap();
    let mut bob = connect(
        &server,
        Login {
            identity: Some(bob_key),
            ..Login::named("Bob")
        },
    )
    .await
    .unwrap();

    assert_eq!(
        bob.ask(Message::Kick {
            client_id: alice.id,
            from_server: true,
            reason: "x".into()
        })
        .await,
        Err("Keine Berechtigung.".into())
    );
    alice
        .ask(Message::CreateChannel {
            name: "Nebenan".into(),
            parent: 0,
            permanent: true,
            password: None,
            max_clients: -1,
            topic: None,
        })
        .await
        .unwrap();
    alice
        .ask(Message::MoveClient {
            client_id: bob.id,
            channel_id: 2,
        })
        .await
        .unwrap();
    let moved = bob
        .event(|m| matches!(m, Message::ClientMoved { .. }))
        .await;
    assert!(
        matches!(moved, Message::ClientMoved { channel_id: 2, invoker_id, .. } if invoker_id == alice.id)
    );

    alice
        .ask(Message::Kick {
            client_id: bob.id,
            from_server: false,
            reason: "Lobby".into(),
        })
        .await
        .unwrap();
    assert!(matches!(
        bob.event(|m| matches!(m, Message::Kicked { .. })).await,
        Message::Kicked {
            from_server: false,
            ..
        }
    ));

    alice
        .ask(Message::Poke {
            client_id: bob.id,
            text: " hey ".into(),
        })
        .await
        .unwrap();
    let poked = bob.event(|m| matches!(m, Message::Poked { .. })).await;
    assert!(matches!(poked, Message::Poked { text, .. } if text == "hey"));

    alice
        .ask(Message::Ban {
            client_id: bob.id,
            reason: "Spam".into(),
            duration_seconds: 0,
        })
        .await
        .unwrap();
    assert!(matches!(
        bob.event(|m| matches!(m, Message::Kicked { .. })).await,
        Message::Kicked {
            from_server: true,
            ..
        }
    ));
    let left = alice
        .event(|m| matches!(m, Message::ClientLeft { .. }))
        .await;
    assert!(matches!(
        left,
        Message::ClientLeft {
            reason: LeaveReason::Banned,
            ..
        }
    ));

    let again = connect(
        &server,
        Login {
            identity: Some(Identity::from_pem(&bob_pem).unwrap()),
            ..Login::named("Bob")
        },
    )
    .await;
    assert!(again.err().unwrap().starts_with("Gebannt bis 9999"));
}

#[tokio::test]
async fn roles_and_state_changes_reach_everyone() {
    let server = TestServer::start(settings());
    let mut alice = connect(&server, Login::named("Alice")).await.unwrap();
    let mut bob = connect(&server, Login::named("Bob")).await.unwrap();

    alice
        .ask(Message::SetRole {
            fingerprint: bob.identity.fingerprint().into(),
            role: Role::Admin,
        })
        .await
        .unwrap();
    let changed = bob
        .event(|m| matches!(m, Message::ClientChanged { .. }))
        .await;
    assert!(
        matches!(changed, Message::ClientChanged { client } if client.role == Role::Admin && client.talk_power == 100)
    );
    // Admins vergeben Rollen unter Admin, aber nie gegen den Besitzer.
    assert_eq!(
        bob.ask(Message::SetRole {
            fingerprint: alice.identity.fingerprint().into(),
            role: Role::Guest
        })
        .await,
        Err("Gleiche oder höhere Rolle.".into())
    );
    assert_eq!(
        bob.ask(Message::SetRole {
            fingerprint: alice.identity.fingerprint().into(),
            role: Role::Admin
        })
        .await,
        Err("Keine Berechtigung.".into())
    );
    assert_eq!(
        alice
            .ask(Message::SetRole {
                fingerprint: "SHA256:unbekannt".into(),
                role: Role::Guest
            })
            .await,
        Err("Unbekannter Schlüssel.".into())
    );

    bob.tell(Message::SetState {
        input_muted: None,
        output_muted: Some(true),
        away: Some(true),
        away_message: Some("Kaffee".into()),
        nickname: Some("alice".into()),
    })
    .await;
    let changed = alice
        .event(|m| matches!(m, Message::ClientChanged { client } if client.away_message.is_some()))
        .await;
    let Message::ClientChanged { client } = changed else {
        unreachable!()
    };
    assert_eq!(
        (
            client.name.as_str(),
            client.output_muted,
            client.away_message.as_deref()
        ),
        ("alice1", true, Some("Kaffee"))
    );
}

#[tokio::test]
async fn unknown_keys_knock_until_a_moderator_lets_them_in() {
    let server = TestServer::start(ServerSettings {
        require_approval: true,
        ..settings()
    });
    // Auf einem frischen Server kommt der erste Schluessel durch.
    let mut owner = connect(&server, Login::named("Owner")).await.unwrap();

    let eve_key = Identity::generate().unwrap();
    let eve_pem = eve_key.to_pem().unwrap();
    let knocked = connect(
        &server,
        Login {
            identity: Some(eve_key),
            ..Login::named("Eve")
        },
    )
    .await;
    assert!(knocked.err().unwrap().starts_with("Du klopfst an"));
    let Message::ApprovalsChanged { pending } = owner
        .event(|m| matches!(m, Message::ApprovalsChanged { .. }))
        .await
    else {
        unreachable!()
    };
    assert_eq!(pending, 1);

    let Message::ApprovalList { entries } = owner.request(Message::ListApprovals).await else {
        panic!("Liste erwartet")
    };
    assert_eq!(entries[0].name, "Eve");
    owner
        .ask(Message::DecideApproval {
            fingerprint: entries[0].fingerprint.clone(),
            allow: true,
        })
        .await
        .unwrap();
    assert!(
        owner
            .ask(Message::DecideApproval {
                fingerprint: "SHA256:x".into(),
                allow: true
            })
            .await
            .is_err()
    );

    let eve = connect(
        &server,
        Login {
            identity: Some(Identity::from_pem(&eve_pem).unwrap()),
            ..Login::named("Eve")
        },
    )
    .await;
    assert_eq!(eve.unwrap().role(), Role::Member);
}

#[tokio::test]
async fn starts_on_the_data_of_the_csharp_server() {
    let dir = tempfile::TempDir::new().unwrap();
    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/csharp");
    let archives = dir.path().join("archives");
    std::fs::create_dir_all(archives.join("channels")).unwrap();
    std::fs::copy(
        fixtures.join("archives/server.sqlar"),
        archives.join("server.sqlar"),
    )
    .unwrap();
    std::fs::copy(
        fixtures.join("archives/channels/1.sqlar"),
        archives.join("channels/1.sqlar"),
    )
    .unwrap();
    std::fs::copy(
        fixtures.join("host/host.crt"),
        dir.path().join("host_cert.pem"),
    )
    .unwrap();
    std::fs::copy(
        fixtures.join("host/host.key"),
        dir.path().join("host_key.pem"),
    )
    .unwrap();
    let csharp_fingerprint =
        std::fs::read_to_string(fixtures.join("host/fingerprint.txt")).unwrap();

    let server = TestServer::start_in(dir.path(), None);
    assert_eq!(server.host_fingerprint(), csharp_fingerprint.trim());
    assert_eq!(server.settings().name, "Testserver");
    let address = server.local_addr().unwrap();
    let host = server.host_fingerprint().to_owned();

    // Freigabe an und Passwort "geheim", wie der C#-Server es speicherte.
    let knocked = connect_to(
        address,
        &host,
        Login {
            password: Some("geheim"),
            ..Login::named("Neu")
        },
    )
    .await;
    assert!(knocked.err().unwrap().starts_with("Du klopfst an"));

    server.stop().await;
    let paths = ServerPaths::new(Some(dir.path().to_owned())).unwrap();
    EchoServer::update_settings(&paths, |s| s.require_approval = false).unwrap();
    let server = TestServer::start_in(dir.path(), None);
    let mut guest = connect_to(
        server.local_addr().unwrap(),
        &host,
        Login {
            password: Some("geheim"),
            ..Login::named("Neu")
        },
    )
    .await
    .unwrap();
    assert_eq!(guest.role(), Role::Guest);
    assert_eq!(guest.channel_names(), ["Lobby", "Geschützt", "Unterkanal"]);

    let everyone = ArchiveRef {
        scope: ChatScope::Server,
        channel: 0,
        peer: String::new(),
    };
    let Message::ChatHistory { messages, .. } = guest
        .request(Message::SyncChat {
            archive: everyone,
            after_seq: 0,
        })
        .await
    else {
        panic!("Verlauf erwartet")
    };
    let texts: Vec<_> = messages.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(
        texts,
        ["Serverchat: erste Zeile", "Zweite Zeile mit Emoji 🙂"]
    );
}
