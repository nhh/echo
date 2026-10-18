//! Jede Nachricht einmal durch Framing und zurueck. Faellt eine aus der
//! Liste, faellt das hier auf: der Zaehler am Ende muss zur Zahl der
//! Varianten passen.

// Hilfsfunktionen ausserhalb von #[test] zaehlen fuer clippy nicht als Test.
#![allow(clippy::unwrap_used)]

use echo_protocol::{
    ApprovalDto, ApprovalState, ArchiveRef, BoardDto, ChannelDto, ChatImageDto, ChatMessageDto,
    ChatScope, ClientDto, DmSummary, Envelope, FileEntryDto, LeaveReason, Message, Role, ShapeDto,
    StreamDto, StreamKind, decode, encode,
};

fn channel() -> ChannelDto {
    ChannelDto {
        id: 2,
        parent: 1,
        order: 0,
        name: "Lobby".into(),
        topic: Some("Thema".into()),
        has_password: true,
        is_default: false,
        permanent: true,
        max_clients: 10,
        needed_talk_power: 0,
    }
}

fn client() -> ClientDto {
    ClientDto {
        id: 7,
        channel: 2,
        name: "Alice".into(),
        fingerprint: "SHA256:alice".into(),
        role: Role::Admin,
        input_muted: false,
        output_muted: true,
        away_message: None,
        talk_power: 5,
    }
}

fn stream() -> StreamDto {
    StreamDto {
        id: 3,
        owner: 7,
        kind: StreamKind::Screen,
        title: "Bildschirm".into(),
    }
}

fn archive() -> ArchiveRef {
    ArchiveRef {
        scope: ChatScope::Private,
        channel: 0,
        peer: "SHA256:bob".into(),
    }
}

fn chat_message() -> ChatMessageDto {
    ChatMessageDto {
        seq: 41,
        time: 1_760_000_000_000,
        author: "SHA256:alice".into(),
        author_name: "Alice".into(),
        text: String::new(),
        image: Some(ChatImageDto {
            id: "img".into(),
            width: 640,
            height: 480,
            size: 1234,
        }),
    }
}

fn shapes() -> Vec<ShapeDto> {
    vec![
        ShapeDto {
            id: "s1".into(),
            json: Some(r#"{"kind":"rect"}"#.into()),
        },
        ShapeDto {
            id: "s2".into(),
            json: None,
        },
    ]
}

#[allow(clippy::too_many_lines)]
fn every_message() -> Vec<Message> {
    vec![
        Message::Open { version: 22 },
        Message::Challenge {
            nonce: "n".into(),
            server_name: "Server".into(),
            version: 22,
            app_version: "0.1.0".into(),
            needs_password: true,
        },
        Message::Hello {
            nickname: "Alice".into(),
            public_key: "pk".into(),
            signature: "sig".into(),
            password_proof: Some("proof".into()),
            default_channel: 2,
            default_channel_password: None,
            input_muted: true,
            output_muted: false,
        },
        Message::Welcome {
            client_id: 7,
            server_name: "Server".into(),
            welcome_message: "Hallo äöü 🎧".into(),
            role: Role::Owner,
            channels: vec![channel()],
            clients: vec![client()],
            streams: vec![stream()],
        },
        Message::Reply {
            ok: false,
            error: Some("nein".into()),
        },
        Message::Ping,
        Message::Pong,
        Message::JoinChannel {
            channel_id: 2,
            password: Some("pw".into()),
        },
        Message::SendChat {
            scope: ChatScope::Server,
            target: 0,
            text: "hi".into(),
        },
        Message::SetState {
            input_muted: Some(true),
            output_muted: None,
            away: Some(false),
            away_message: None,
            nickname: Some("Al".into()),
        },
        Message::Poke {
            client_id: 8,
            text: "hey".into(),
        },
        Message::Kick {
            client_id: 8,
            from_server: true,
            reason: "spam".into(),
        },
        Message::Ban {
            client_id: 8,
            reason: "spam".into(),
            duration_seconds: 3600,
        },
        Message::CreateChannel {
            name: "Neu".into(),
            parent: 0,
            permanent: false,
            password: None,
            max_clients: 0,
            topic: None,
        },
        Message::EditChannel {
            channel_id: 2,
            name: "Lobby".into(),
            topic: None,
            change_password: true,
            password: Some(String::new()),
            needed_talk_power: 3,
        },
        Message::DeleteChannel { channel_id: 2 },
        Message::MoveClient {
            client_id: 8,
            channel_id: 3,
        },
        Message::SetRole {
            fingerprint: "SHA256:bob".into(),
            role: Role::Member,
        },
        Message::ListFiles {
            channel: 2,
            folder: String::new(),
        },
        Message::FileList {
            channel: 2,
            folder: "a/b".into(),
            entries: vec![FileEntryDto {
                id: "f".into(),
                name: "x.bin".into(),
                is_folder: false,
                size: 10,
                uploaded_by_name: "Alice".into(),
                uploaded_at: 1_760_000_000_000,
            }],
            used: 10,
            limit: 100,
        },
        Message::CreateFolder {
            channel: 2,
            folder: String::new(),
            name: "a".into(),
        },
        Message::RenameFile {
            file_id: "f".into(),
            name: "y.bin".into(),
        },
        Message::DeleteFile {
            file_id: "f".into(),
        },
        Message::MoveFile {
            file_id: "f".into(),
            channel: 3,
            folder: "a".into(),
        },
        Message::FilesChanged { channel: 2 },
        Message::RequestUpload {
            channel: 2,
            folder: String::new(),
            name: "x".into(),
            size: 5,
        },
        Message::RequestDownload {
            file_id: "f".into(),
            offset: 3,
            stream: true,
        },
        Message::RequestChatImage {
            scope: ChatScope::Channel,
            target: 0,
            size: 99,
            width: 10,
            height: 20,
        },
        Message::RequestImageDownload {
            archive: archive(),
            image_id: "img".into(),
        },
        Message::SyncChat {
            archive: archive(),
            after_seq: 40,
        },
        Message::LoadHistory {
            archive: archive(),
            before_seq: 40,
        },
        Message::ChatHistory {
            archive: archive(),
            messages: vec![chat_message()],
            more: true,
        },
        Message::DmConversations {
            conversations: vec![DmSummary {
                peer: "SHA256:bob".into(),
                peer_name: "Bob".into(),
                last_seq: 3,
            }],
        },
        Message::ListBoards { channel: 2 },
        Message::BoardList {
            channel: 2,
            boards: vec![BoardDto {
                id: "b".into(),
                name: "Tafel".into(),
            }],
        },
        Message::CreateBoard {
            channel: 2,
            name: "Tafel".into(),
        },
        Message::RenameBoard {
            board_id: "b".into(),
            name: "Neu".into(),
        },
        Message::DeleteBoard {
            board_id: "b".into(),
        },
        Message::BoardsChanged { channel: 2 },
        Message::OpenBoard {
            board_id: "b".into(),
        },
        Message::BoardSnapshot {
            board_id: "b".into(),
            name: "Tafel".into(),
            participants: vec![7],
        },
        Message::CloseBoard {
            board_id: "b".into(),
        },
        Message::JoinBoard {
            board_id: "b".into(),
            join: true,
        },
        Message::BoardPresence {
            board_id: "b".into(),
            participants: vec![7, 8],
        },
        Message::EditBoard {
            board_id: "b".into(),
            changes: shapes(),
        },
        Message::BoardEdited {
            board_id: "b".into(),
            by: 7,
            changes: shapes(),
        },
        Message::BoardPointer {
            board_id: "b".into(),
            x: 1.5,
            y: -2.0,
            laser: true,
        },
        Message::BoardPointerMoved {
            board_id: "b".into(),
            by: 7,
            x: 1.5,
            y: -2.0,
            laser: false,
        },
        Message::RequestStream {
            kind: StreamKind::Kino,
            title: "Film".into(),
        },
        Message::StreamTicket {
            token: "t".into(),
            stream_id: 3,
        },
        Message::StreamHello { token: "t".into() },
        Message::TransferTicket {
            token: "t".into(),
            name: "x".into(),
            size: 5,
        },
        Message::TransferHello { token: "t".into() },
        Message::ListApprovals,
        Message::ApprovalList {
            entries: vec![ApprovalDto {
                fingerprint: "SHA256:eve".into(),
                name: "Eve".into(),
                requested: 1_760_000_000_000,
                state: ApprovalState::Pending,
            }],
        },
        Message::DecideApproval {
            fingerprint: "SHA256:eve".into(),
            allow: false,
        },
        Message::ApprovalsChanged { pending: 1 },
        Message::ChannelAdded { channel: channel() },
        Message::ChannelRemoved { channel_id: 2 },
        Message::ChannelChanged { channel: channel() },
        Message::ClientJoined { client: client() },
        Message::ClientLeft {
            client_id: 7,
            reason: LeaveReason::Banned,
            detail: Some("x".into()),
        },
        Message::ClientMoved {
            client_id: 7,
            channel_id: 3,
            invoker_id: 1,
        },
        Message::ClientChanged { client: client() },
        Message::StreamStarted { stream: stream() },
        Message::StreamEnded { stream_id: 3 },
        Message::KeyframeRequest { stream_id: 3 },
        Message::WatchStream { stream_id: 3 },
        Message::UnwatchStream { stream_id: 3 },
        Message::ChatEvent {
            archive: archive(),
            from_id: 7,
            message: chat_message(),
        },
        Message::Poked {
            from_id: 7,
            from_name: "Alice".into(),
            text: "hey".into(),
        },
        Message::Kicked {
            from_server: false,
            reason: "weg".into(),
            invoker_id: 1,
            invoker_name: None,
        },
    ]
}

#[test]
fn every_message_survives_the_wire() {
    let messages = every_message();
    // 73 Nachrichten im C#-Vorgaenger, ohne DatagramOffer (echo-231).
    assert_eq!(messages.len(), 72);
    for (index, message) in messages.into_iter().enumerate() {
        let envelope = Envelope {
            request_id: u32::try_from(index).unwrap() + 1,
            message,
        };
        let frame = encode(&envelope).unwrap();
        let length = u32::from_le_bytes(frame[..4].try_into().unwrap());
        assert_eq!(length as usize, frame.len() - 4);
        assert_eq!(decode(&frame[4..]).unwrap(), envelope);
    }
}
