//! Tafeln mit echten QUIC-Clients: ansehen, mitmachen, zeichnen, Zeiger.

// Szenarien lesen sich am besten am Stueck.
#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

mod common;

use common::{Login, TestServer, connect, settings};
use echo_protocol::{Message, ShapeDto};

fn shape(id: &str, json: Option<&str>) -> ShapeDto {
    ShapeDto {
        id: id.into(),
        json: json.map(str::to_owned),
    }
}

#[tokio::test]
async fn viewers_see_live_what_participants_draw() {
    let server = TestServer::start(settings());
    let mut alice = connect(&server, Login::named("Alice")).await.unwrap();
    let mut bob = connect(&server, Login::named("Bob")).await.unwrap();

    alice
        .ask(Message::CreateBoard {
            channel: 1,
            name: "  Skizze  ".into(),
        })
        .await
        .unwrap();
    assert!(matches!(
        bob.event(|m| matches!(m, Message::BoardsChanged { .. }))
            .await,
        Message::BoardsChanged { channel: 1 }
    ));
    let Message::BoardList { boards, .. } = bob.request(Message::ListBoards { channel: 1 }).await
    else {
        panic!()
    };
    assert_eq!(boards[0].name, "Skizze");
    let board = boards[0].id.clone();

    let Message::BoardSnapshot { participants, .. } = alice
        .request(Message::OpenBoard {
            board_id: board.clone(),
        })
        .await
    else {
        panic!()
    };
    assert!(participants.is_empty());
    assert!(matches!(
        bob.request(Message::OpenBoard {
            board_id: board.clone()
        })
        .await,
        Message::BoardSnapshot { .. }
    ));

    assert_eq!(
        alice
            .ask(Message::EditBoard {
                board_id: board.clone(),
                changes: vec![shape("s1", Some("{}"))]
            })
            .await,
        Err("Erst mitmachen, dann zeichnen.".into())
    );
    alice
        .ask(Message::JoinBoard {
            board_id: board.clone(),
            join: true,
        })
        .await
        .unwrap();
    let presence = bob
        .event(|m| matches!(m, Message::BoardPresence { .. }))
        .await;
    assert!(
        matches!(presence, Message::BoardPresence { participants, .. } if participants == [alice.id])
    );

    alice
        .ask(Message::EditBoard {
            board_id: board.clone(),
            changes: vec![
                shape("s1", Some(r#"{"kind":"rect"}"#)),
                shape("s2", Some("{}")),
            ],
        })
        .await
        .unwrap();
    let edited = bob
        .event(|m| matches!(m, Message::BoardEdited { .. }))
        .await;
    assert!(
        matches!(edited, Message::BoardEdited { by, changes, .. } if by == alice.id && changes.len() == 2)
    );
    alice
        .ask(Message::EditBoard {
            board_id: board.clone(),
            changes: vec![shape("s2", None)],
        })
        .await
        .unwrap();
    assert_eq!(
        alice
            .ask(Message::EditBoard {
                board_id: board.clone(),
                changes: vec![shape("../x", None)]
            })
            .await,
        Err("Ungültige Änderung.".into())
    );

    alice
        .tell(Message::BoardPointer {
            board_id: board.clone(),
            x: 1.0,
            y: 2.0,
            laser: true,
        })
        .await;
    alice
        .tell(Message::BoardPointer {
            board_id: board.clone(),
            x: f32::NAN,
            y: 2.0,
            laser: false,
        })
        .await;
    let pointer = bob
        .event(|m| matches!(m, Message::BoardPointerMoved { .. }))
        .await;
    assert!(
        matches!(pointer, Message::BoardPointerMoved { x, laser: true, .. } if (x - 1.0).abs() < f32::EPSILON)
    );

    // Wer neu oeffnet, bekommt den Stand: nur s1 ist noch da.
    let mut carol = connect(&server, Login::named("Carol")).await.unwrap();
    assert!(
        matches!(carol.request(Message::OpenBoard { board_id: board.clone() }).await,
        Message::BoardSnapshot { participants, .. } if participants == [alice.id])
    );
    let Message::BoardEdited { by: 0, changes, .. } = carol
        .event(|m| matches!(m, Message::BoardEdited { .. }))
        .await
    else {
        panic!("Formen beim Oeffnen erwartet")
    };
    assert_eq!(changes, [shape("s1", Some(r#"{"kind":"rect"}"#))]);

    // Wer den Kanal verlaesst, sieht die Tafel nicht mehr und macht nicht mehr mit.
    alice
        .ask(Message::CreateChannel {
            name: "Weg".into(),
            parent: 0,
            permanent: true,
            password: None,
            max_clients: -1,
            topic: None,
        })
        .await
        .unwrap();
    alice
        .ask(Message::JoinChannel {
            channel_id: 2,
            password: None,
        })
        .await
        .unwrap();
    let left = bob
        .event(
            |m| matches!(m, Message::BoardPresence { participants, .. } if participants.is_empty()),
        )
        .await;
    assert!(matches!(left, Message::BoardPresence { .. }));
    assert_eq!(
        alice.ask(Message::ListBoards { channel: 1 }).await,
        Err("Die Tafeln eines Kanals sieht nur, wer darin ist.".into())
    );
}

#[tokio::test]
async fn boards_are_managed_by_their_creators_and_moderators() {
    let server = TestServer::start(settings());
    let mut alice = connect(&server, Login::named("Alice")).await.unwrap();
    let mut bob = connect(&server, Login::named("Bob")).await.unwrap();

    alice
        .ask(Message::CreateBoard {
            channel: 1,
            name: "Alice".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        alice
            .ask(Message::CreateBoard {
                channel: 1,
                name: "   ".into()
            })
            .await,
        Err("Ungültiger Name.".into())
    );
    let Message::BoardList { boards, .. } = bob.request(Message::ListBoards { channel: 1 }).await
    else {
        panic!()
    };
    let board = boards[0].id.clone();

    assert_eq!(
        bob.ask(Message::RenameBoard {
            board_id: board.clone(),
            name: "Bob".into()
        })
        .await,
        Err("Keine Berechtigung.".into())
    );
    alice
        .ask(Message::RenameBoard {
            board_id: board.clone(),
            name: "Neu".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        bob.ask(Message::JoinBoard {
            board_id: board.clone(),
            join: true
        })
        .await,
        Err("Die Tafel ist nicht offen.".into())
    );
    bob.request(Message::OpenBoard {
        board_id: board.clone(),
    })
    .await;
    bob.ask(Message::JoinBoard {
        board_id: board.clone(),
        join: true,
    })
    .await
    .unwrap();
    bob.ask(Message::CloseBoard {
        board_id: board.clone(),
    })
    .await
    .unwrap();

    assert_eq!(
        bob.ask(Message::DeleteBoard {
            board_id: board.clone()
        })
        .await,
        Err("Keine Berechtigung.".into())
    );
    alice
        .ask(Message::DeleteBoard {
            board_id: board.clone(),
        })
        .await
        .unwrap();
    assert_eq!(
        bob.ask(Message::OpenBoard { board_id: board }).await,
        Err("Die Tafel gibt es nicht mehr.".into())
    );

    // Tafeln ueberleben einen Neustart, mit ihren Formen.
    alice
        .ask(Message::CreateBoard {
            channel: 1,
            name: "Bleibt".into(),
        })
        .await
        .unwrap();
    let data = server.dir.path().to_owned();
    let TestServer {
        server: running,
        dir,
    } = server;
    drop((alice, bob));
    running.stop().await;
    let restarted = TestServer {
        server: TestServer::start_in(&data, None),
        dir,
    };
    let mut carol = connect(&restarted, Login::named("Carol")).await.unwrap();
    let Message::BoardList { boards, .. } = carol.request(Message::ListBoards { channel: 1 }).await
    else {
        panic!()
    };
    assert_eq!(boards[0].name, "Bleibt");
    assert!(matches!(
        carol
            .request(Message::OpenBoard {
                board_id: boards[0].id.clone()
            })
            .await,
        Message::BoardSnapshot { .. }
    ));
}
