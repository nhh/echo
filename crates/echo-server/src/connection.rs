//! Eine QUIC-Verbindung zu einem Client: Handschlag auf dem ersten
//! bidirektionalen Stream, danach dort Steuernachrichten in beide
//! Richtungen. Sprache kommt als Datagramm, sonst als unidirektionaler
//! Stream je Frame; beides geht ohne Sperre an die Kanalmitglieder weiter.
//! Sendungen laufen ueber eine Queue mit genau einem Schreiber.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use echo_protocol::{
    Envelope, FrameKind, Identity, LeaveReason, Message, VoiceFrame, Wire, hello_payload,
    new_nonce, read_message, write_message,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::client::{Client, ClientLink, Frame};
use crate::state::{Admission, ChannelEdit, ChannelRequest, ServerState, StateChange};
use crate::state::{ImageRequest, UploadRequest};
use crate::transfer;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// So lange bekommt ein abgewiesener Client, um den Grund zu lesen und selbst
/// aufzulegen; direkt aufgelegt, las er oft nur "aborted" statt des Grundes
/// (echo-102).
const REFUSAL_GRACE: Duration = Duration::from_secs(2);

/// Groesster Sprachrahmen auf der Leitung.
const MAX_VOICE_PACKET: usize = Wire::MAX_VOICE_FRAME_BYTES + VoiceFrame::HEADER_SIZE;

/// Steuernachrichten, die auf einen langsamen Client warten duerfen.
const OUTBOX_CAPACITY: usize = 512;

/// Die Programmversion des Servers; sie steht in Fehlermeldungen, damit man
/// weiss, welche Seite aelter ist.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Wird ein Stream oder eine Nachricht nicht gelesen, liegt es am Client.
#[derive(Debug, thiserror::Error)]
enum ConnectionError {
    #[error("Zeit fuer den Handschlag abgelaufen")]
    Timeout,
    #[error("Verbindung: {0}")]
    Quic(#[from] quinn::ConnectionError),
    #[error("Protokoll: {0}")]
    Protocol(#[from] echo_protocol::ProtocolError),
    #[error("Stream: {0}")]
    Write(#[from] quinn::WriteError),
    #[error("{0}")]
    Unexpected(&'static str),
}

impl From<tokio::time::error::Elapsed> for ConnectionError {
    fn from(_: tokio::time::error::Elapsed) -> Self {
        Self::Timeout
    }
}

/// Fuehrt eine Verbindung vom Handschlag bis zum Ende.
pub(crate) async fn run(
    connection: quinn::Connection,
    state: Arc<ServerState>,
    host_fingerprint: Arc<str>,
    stopping: CancellationToken,
) {
    let remote = connection.remote_address();
    let closing = stopping.child_token();
    match accept(&connection, &state, &host_fingerprint, &closing).await {
        Ok(Some((client, send, recv, outbox))) => {
            serve(&connection, &state, &client, send, recv, outbox, &closing).await;
            let (reason, detail) = if stopping.is_cancelled() {
                (LeaveReason::ServerStopped, None)
            } else {
                client.leave_reason()
            };
            state.remove(&client, reason, detail);
        }
        Ok(None) => {}
        Err(error) => state.log(&format!("Handschlag von {remote} gescheitert: {error}")),
    }
    connection.close(0u32.into(), b"");
}

type Accepted = (
    Arc<Client>,
    quinn::SendStream,
    quinn::RecvStream,
    mpsc::Receiver<Frame>,
);

/// Der Handschlag: `Open`, `Challenge`, `Hello`, `Welcome`. `None`, wenn der
/// Client abgewiesen wurde; der Grund ist dann schon unterwegs.
async fn accept(
    connection: &quinn::Connection,
    state: &Arc<ServerState>,
    host_fingerprint: &str,
    closing: &CancellationToken,
) -> Result<Option<Accepted>, ConnectionError> {
    let (mut send, mut recv) =
        tokio::time::timeout(HANDSHAKE_TIMEOUT, connection.accept_bi()).await??;
    let first = tokio::time::timeout(HANDSHAKE_TIMEOUT, read_message(&mut recv))
        .await??
        .ok_or(ConnectionError::Unexpected("Stream endete vor Open"))?;
    if let Message::TransferHello { token } = &first.message {
        // Eine Datenverbindung, kein Client: Datei uebertragen und auflegen.
        transfer::run(state, send, recv, first.request_id, token).await;
        return Ok(None);
    }
    let Some(nonce) = challenge(state, &mut send, &mut recv, first).await? else {
        return Ok(None);
    };
    let Some((admission, request_id)) =
        read_hello(&mut send, &mut recv, nonce, host_fingerprint).await?
    else {
        return Ok(None);
    };

    let (outbox_sender, outbox) = mpsc::channel(OUTBOX_CAPACITY);
    let link = ClientLink {
        connection: connection.clone(),
        outbox: outbox_sender,
        closing: closing.clone(),
    };
    match state.admit(&admission, link) {
        Ok((client, welcome)) => {
            let reply = Envelope {
                request_id,
                message: welcome,
            };
            write_message(&mut send, &reply).await?;
            state.greet(&client);
            Ok(Some((client, send, recv, outbox)))
        }
        Err(reason) => {
            refuse(&mut send, &mut recv, request_id, reason).await?;
            Ok(None)
        }
    }
}

/// Prueft die Version aus `Open` und schickt die `Challenge`; liefert ihre
/// Nonce, oder `None` nach einer Ablehnung.
async fn challenge(
    state: &ServerState,
    send: &mut quinn::SendStream,
    recv: &mut quinn::RecvStream,
    open: Envelope,
) -> Result<Option<String>, ConnectionError> {
    let version = match open.message {
        Message::Open { version } => Some(version),
        Message::StreamHello { .. } => {
            // Medien kommen in einer eigenen Etappe des Ports.
            refuse(
                send,
                recv,
                open.request_id,
                "Noch nicht unterstützt.".into(),
            )
            .await?;
            return Ok(None);
        }
        _ => None,
    };
    if version != Some(Wire::VERSION) {
        refuse(send, recv, open.request_id, version_mismatch(version)).await?;
        return Ok(None);
    }

    let nonce = new_nonce()?;
    let challenge = Message::Challenge {
        nonce: nonce.clone(),
        server_name: state.server_name(),
        version: Wire::VERSION,
        app_version: SERVER_VERSION.into(),
        needs_password: state.needs_password(),
    };
    write_message(
        send,
        &Envelope {
            request_id: open.request_id,
            message: challenge,
        },
    )
    .await?;
    Ok(Some(nonce))
}

/// Liest `Hello` und prueft die Signatur; liefert, was der Client verlangt,
/// mit der Nummer seiner Anfrage, oder `None` nach einer Ablehnung.
async fn read_hello(
    send: &mut quinn::SendStream,
    recv: &mut quinn::RecvStream,
    nonce: String,
    host_fingerprint: &str,
) -> Result<Option<(Admission, u32)>, ConnectionError> {
    let hello = tokio::time::timeout(HANDSHAKE_TIMEOUT, read_message(recv))
        .await??
        .ok_or(ConnectionError::Unexpected("Stream endete vor Hello"))?;
    let Message::Hello {
        nickname,
        public_key,
        signature,
        password_proof,
        default_channel,
        default_channel_password,
        input_muted,
        output_muted,
    } = hello.message
    else {
        return Err(ConnectionError::Unexpected("Hello erwartet"));
    };

    let payload = hello_payload(&nonce, host_fingerprint, &nickname);
    let fingerprint = match Identity::fingerprint_of(&public_key) {
        Ok(fingerprint) if Identity::verify(&public_key, &payload, &signature) => fingerprint,
        _ => {
            refuse(send, recv, hello.request_id, "Signatur ungültig.".into()).await?;
            return Ok(None);
        }
    };
    let admission = Admission {
        fingerprint,
        nickname,
        password_proof,
        default_channel,
        default_channel_password,
        input_muted,
        output_muted,
        nonce,
    };
    Ok(Some((admission, hello.request_id)))
}

/// Schreibt die Ablehnung und wartet, bis der Client auflegt, hoechstens
/// [`REFUSAL_GRACE`]; sonst verwirft das Schliessen, was noch unterwegs ist.
async fn refuse(
    send: &mut quinn::SendStream,
    recv: &mut quinn::RecvStream,
    request_id: u32,
    reason: String,
) -> Result<(), ConnectionError> {
    let reply = Envelope {
        request_id,
        message: Message::Reply {
            ok: false,
            error: Some(reason),
        },
    };
    write_message(send, &reply).await?;
    let _ = send.finish();
    let _ = tokio::time::timeout(REFUSAL_GRACE, async {
        let mut rest = [0u8; 64];
        while let Ok(Some(_)) = recv.read(&mut rest).await {}
    })
    .await;
    Ok(())
}

/// Sagt, welche Seite aelter ist und was zu tun ist; der Nutzer sieht den
/// Text unveraendert im Client.
fn version_mismatch(client: Option<u32>) -> String {
    let server = format!(
        "Der Server spricht Protokoll {} (echo-server {SERVER_VERSION})",
        Wire::VERSION
    );
    match client {
        None => format!("{server}. Der Client hat sich nicht als echo-Client gemeldet."),
        Some(version) if version < Wire::VERSION => format!(
            "{server}, der Client Protokoll {version}. Der Client ist älter — bitte aktualisieren."
        ),
        Some(version) => format!(
            "{server}, der Client Protokoll {version}. Der Server ist älter — bitte den Server aktualisieren."
        ),
    }
}

/// Laeuft, bis eine Seite aufgibt: Steuerkanal, Sendequeue, Sprache.
async fn serve(
    connection: &quinn::Connection,
    state: &Arc<ServerState>,
    client: &Arc<Client>,
    send: quinn::SendStream,
    recv: quinn::RecvStream,
    outbox: mpsc::Receiver<Frame>,
    closing: &CancellationToken,
) {
    tokio::select! {
        () = closing.cancelled() => {}
        () = send_loop(send, outbox) => {}
        () = control_loop(recv, state, client) => {}
        () = datagram_loop(connection, state, client) => {}
        () = voice_stream_loop(connection, state, client) => {}
    }
}

async fn send_loop(mut send: quinn::SendStream, mut outbox: mpsc::Receiver<Frame>) {
    while let Some(frame) = outbox.recv().await {
        if send.write_all(&frame).await.is_err() {
            return;
        }
    }
}

async fn control_loop(mut recv: quinn::RecvStream, state: &Arc<ServerState>, client: &Client) {
    while let Ok(Some(envelope)) = read_message(&mut recv).await {
        handle(state, client, envelope);
    }
}

/// Eine Anfrage ausfuehren. Anfragen mit `request_id` bekommen immer eine
/// Antwort: ihr Ergebnis oder `Reply`.
fn handle(state: &Arc<ServerState>, client: &Client, envelope: Envelope) {
    let id = envelope.request_id;
    let result = answer(state, client, envelope.message);
    match result {
        Ok(Some(answer)) => client.reply(id, answer),
        Ok(None) if id > 0 => client.reply(
            id,
            Message::Reply {
                ok: true,
                error: None,
            },
        ),
        Err(error) if id > 0 => client.reply(
            id,
            Message::Reply {
                ok: false,
                error: Some(error),
            },
        ),
        Ok(None) | Err(_) => {}
    }
}

/// `Ok(Some)` ist eine eigene Antwort, `Ok(None)` ein schlichtes "erledigt".
type Answer = Result<Option<Message>, String>;

fn answer(state: &Arc<ServerState>, client: &Client, message: Message) -> Answer {
    match message {
        Message::Ping => Ok(Some(Message::Pong)),
        Message::SendChat {
            scope,
            target,
            text,
        } => {
            state.chat(client, scope, target, &text);
            Ok(None)
        }
        Message::SyncChat { archive, after_seq } => {
            state.history(client, archive, after_seq, 0).map(Some)
        }
        Message::LoadHistory {
            archive,
            before_seq,
        } => state.history(client, archive, 0, before_seq).map(Some),
        Message::SetState {
            input_muted,
            output_muted,
            away,
            away_message,
            nickname,
        } => {
            let change = StateChange {
                input_muted,
                output_muted,
                away,
                away_message,
                nickname,
            };
            state.set_state(client, change);
            Ok(None)
        }
        Message::Poke { client_id, text } => done(state.poke(client, client_id, &text)),
        Message::ListApprovals => state.list_approvals(client).map(Some),
        Message::DecideApproval { fingerprint, allow } => {
            done(state.decide_approval(client, &fingerprint, allow))
        }
        request @ (Message::ListFiles { .. }
        | Message::CreateFolder { .. }
        | Message::RenameFile { .. }
        | Message::MoveFile { .. }
        | Message::DeleteFile { .. }
        | Message::RequestUpload { .. }
        | Message::RequestDownload { .. }
        | Message::RequestChatImage { .. }
        | Message::RequestImageDownload { .. }) => file_request(state, client, request),
        request @ (Message::JoinChannel { .. }
        | Message::CreateChannel { .. }
        | Message::EditChannel { .. }
        | Message::DeleteChannel { .. }) => channel_request(state, client, request),
        request @ (Message::Kick { .. }
        | Message::Ban { .. }
        | Message::MoveClient { .. }
        | Message::SetRole { .. }) => moderation_request(state, client, request),
        _ => Err(UNKNOWN.into()),
    }
}

const UNKNOWN: &str = "Unbekannte Anfrage.";

/// Kanaele betreten, anlegen, aendern, loeschen.
fn channel_request(state: &ServerState, client: &Client, message: Message) -> Answer {
    match message {
        Message::JoinChannel {
            channel_id,
            password,
        } => done(state.join(client, channel_id, password.as_deref())),
        Message::CreateChannel {
            name,
            parent,
            permanent,
            password,
            max_clients,
            topic,
        } => {
            let request = ChannelRequest {
                name,
                parent,
                permanent,
                password,
                max_clients,
                topic,
            };
            done(state.create_channel(client, request))
        }
        Message::EditChannel {
            channel_id,
            name,
            topic,
            change_password,
            password,
            needed_talk_power,
        } => {
            let edit = ChannelEdit {
                channel_id,
                name,
                topic,
                change_password,
                password,
                needed_talk_power,
            };
            done(state.edit_channel(client, edit))
        }
        Message::DeleteChannel { channel_id } => done(state.delete_channel(client, channel_id)),
        _ => Err(UNKNOWN.into()),
    }
}

/// Werfen, bannen, verschieben, Rollen.
fn moderation_request(state: &ServerState, client: &Client, message: Message) -> Answer {
    match message {
        Message::Kick {
            client_id,
            from_server,
            reason,
        } => done(state.kick(client, client_id, from_server, &reason)),
        Message::Ban {
            client_id,
            reason,
            duration_seconds,
        } => done(state.ban(client, client_id, &reason, duration_seconds)),
        Message::MoveClient {
            client_id,
            channel_id,
        } => done(state.move_client(client, client_id, channel_id)),
        Message::SetRole { fingerprint, role } => done(state.set_role(client, &fingerprint, role)),
        _ => Err(UNKNOWN.into()),
    }
}

/// Dateibereich, Uebertragungen und Chatbilder.
fn file_request(state: &Arc<ServerState>, client: &Client, message: Message) -> Answer {
    match message {
        Message::ListFiles { channel, folder } => state.list_files(channel, &folder).map(Some),
        Message::CreateFolder {
            channel,
            folder,
            name,
        } => done(state.create_folder(client, channel, &folder, &name)),
        Message::RenameFile { file_id, name } => done(state.rename_file(client, &file_id, &name)),
        Message::MoveFile {
            file_id,
            channel,
            folder,
        } => done(state.move_file(client, &file_id, channel, &folder)),
        Message::DeleteFile { file_id } => done(state.delete_file(client, &file_id)),
        Message::RequestUpload {
            channel,
            folder,
            name,
            size,
        } => state
            .begin_upload(
                client,
                UploadRequest {
                    channel,
                    folder,
                    name,
                    size,
                },
            )
            .map(Some),
        Message::RequestDownload {
            file_id,
            offset,
            stream,
        } => state
            .begin_download(client, &file_id, offset, stream)
            .map(Some),
        Message::RequestChatImage {
            scope,
            target,
            size,
            width,
            height,
        } => state
            .begin_chat_image(
                client,
                &ImageRequest {
                    scope,
                    target,
                    size,
                    width,
                    height,
                },
            )
            .map(Some),
        Message::RequestImageDownload { archive, image_id } => state
            .begin_image_download(client, &archive, &image_id)
            .map(Some),
        _ => Err(UNKNOWN.into()),
    }
}

fn done(result: Result<(), String>) -> Answer {
    result.map(|()| None)
}

async fn datagram_loop(connection: &quinn::Connection, state: &ServerState, client: &Client) {
    while let Ok(datagram) = connection.read_datagram().await {
        forward_voice(state, client, &datagram);
    }
}

/// Sprache als eigener Stream je Frame, wenn Datagramme nicht gehen.
async fn voice_stream_loop(
    connection: &quinn::Connection,
    state: &Arc<ServerState>,
    client: &Arc<Client>,
) {
    while let Ok(mut stream) = connection.accept_uni().await {
        let state = Arc::clone(state);
        let client = Arc::clone(client);
        tokio::spawn(async move {
            // Ein abgebrochener Frame ist ein verlorener Frame, mehr nicht.
            if let Ok(bytes) = stream.read_to_end(MAX_VOICE_PACKET).await {
                forward_voice(&state, &client, &Bytes::from(bytes));
            }
        });
    }
}

/// An alle im selben Kanal, ohne Sperre aus dem Schnappschuss; der Sender
/// bleibt aussen vor, seine eigene Stimme waere ein Echo (echo-47). Nur
/// Sprache und nur mit offenem Mikrofon; Musik kommt ueber ihre eigenen Wege.
fn forward_voice(state: &ServerState, client: &Client, packet: &Bytes) {
    if packet.len() > MAX_VOICE_PACKET || client.input_muted() {
        return;
    }
    let Ok(frame) = VoiceFrame::parse(packet) else {
        return;
    };
    if frame.kind != FrameKind::Voice {
        return;
    }

    let mut stamped = packet.to_vec();
    if VoiceFrame::stamp_sender(&mut stamped, client.id()).is_err() {
        return;
    }
    let stamped = Bytes::from(stamped);
    let members = state.members();
    if let Some(listeners) = members.get(&client.channel()) {
        for listener in listeners {
            if listener.id() != client.id() && !listener.output_muted() {
                listener.send_voice(stamped.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_mismatch_says_which_side_is_older() {
        assert!(version_mismatch(None).contains("nicht als echo-Client"));
        assert!(version_mismatch(Some(1)).contains("Client ist älter"));
        assert!(version_mismatch(Some(Wire::VERSION + 1)).contains("Server ist älter"));
    }
}
