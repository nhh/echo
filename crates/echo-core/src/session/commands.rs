//! Befehle an den Server. Jeder liefert den Fehler als Wert; die
//! Befehle der Oberflaeche melden ihn zusaetzlich als `Error`-Ereignis fuer
//! den Toast.

use std::sync::atomic::Ordering;

use echo_protocol::{BoardDto, ChatScope, Envelope, FileEntryDto, Message, Role, ShapeDto};
use tokio::sync::oneshot;

use super::{REQUEST_TIMEOUT, Session, frame};
use crate::events::EventKind;

/// Der Inhalt eines Ordners im Dateibereich (echo-20); `used` und `limit`
/// gelten fuer den ganzen Server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileListing {
    pub channel: u64,
    pub folder: String,
    pub entries: Vec<FileEntryDto>,
    pub used: u64,
    pub limit: u64,
}

const NOT_CONNECTED: &str = "Nicht verbunden.";
const UNEXPECTED: &str = "Unerwartete Antwort.";

impl Session {
    // --- Grundlagen -----------------------------------------------------------

    /// Eine Anfrage mit Antwort. Eine Ablehnung des Servers kommt als
    /// `Reply { ok: false }` zurueck, nicht als `Err`.
    pub(super) async fn ask(&self, message: Message) -> Result<Message, String> {
        let live = self.live().ok_or(NOT_CONNECTED)?;
        let request_id = live.next_request.fetch_add(1, Ordering::Relaxed);
        let bytes = frame(&Envelope {
            request_id,
            message,
        })
        .ok_or("Nachricht zu groß.")?;
        let (sender, receiver) = oneshot::channel();
        live.pending.lock().insert(request_id, sender);
        if live.outbox.send(bytes).is_err() {
            live.pending.lock().remove(&request_id);
            return Err(NOT_CONNECTED.into());
        }
        let answer = tokio::time::timeout(REQUEST_TIMEOUT, receiver).await;
        live.pending.lock().remove(&request_id);
        match answer {
            Ok(Ok(message)) => Ok(message),
            Ok(Err(_)) => Err(NOT_CONNECTED.into()),
            Err(_) => Err("Der Server antwortet nicht.".into()),
        }
    }

    /// Eine Anfrage, deren Antwort nur `Reply` ist.
    async fn request(&self, message: Message) -> Result<(), String> {
        match self.ask(message).await? {
            Message::Reply { ok: true, .. } => Ok(()),
            answer => Err(refusal(answer)),
        }
    }

    /// Wie `request`, meldet den Fehler aber auch als Toast.
    async fn run(&self, message: Message, what: &str) -> Result<(), String> {
        self.request(message).await.inspect_err(|error| {
            self.publish(EventKind::Error(format!("{what}: {error}")));
        })
    }

    /// Eine Nachricht ohne Antwort.
    fn tell(&self, message: Message) {
        if let Some(live) = self.live()
            && let Some(bytes) = frame(&Envelope::event(message))
        {
            // Ohne Leitung ist die Nachricht verloren; sie war fluechtig.
            let _ = live.outbox.send(bytes);
        }
    }

    // --- Kanaele und Nutzer ---------------------------------------------------

    pub async fn join_channel(
        &self,
        channel_id: u64,
        password: Option<String>,
    ) -> Result<(), String> {
        self.run(
            Message::JoinChannel {
                channel_id,
                password,
            },
            "Kanal wechseln",
        )
        .await
    }

    pub async fn send_channel_message(&self, text: &str) -> Result<(), String> {
        self.send_chat(ChatScope::Channel, 0, text).await
    }

    pub async fn send_server_message(&self, text: &str) -> Result<(), String> {
        self.send_chat(ChatScope::Server, 0, text).await
    }

    /// Die eigene Zeile kommt als `Chat` vom Server zurueck, mit ihrer Nummer.
    pub async fn send_private_message(&self, client_id: u16, text: &str) -> Result<(), String> {
        self.send_chat(ChatScope::Private, client_id, text).await
    }

    async fn send_chat(&self, scope: ChatScope, target: u16, text: &str) -> Result<(), String> {
        let message = Message::SendChat {
            scope,
            target,
            text: text.to_owned(),
        };
        self.run(message, "Nachricht senden").await
    }

    pub async fn set_input_muted(&self, muted: bool) -> Result<(), String> {
        self.run(state(Some(muted), None, None), "Mikrofon").await
    }

    pub async fn set_output_muted(&self, muted: bool) -> Result<(), String> {
        self.run(state(None, Some(muted), None), "Lautsprecher")
            .await
    }

    pub async fn set_away(&self, away: bool, message: &str) -> Result<(), String> {
        let request = Message::SetState {
            input_muted: None,
            output_muted: None,
            away: Some(away),
            away_message: Some(message.to_owned()),
            nickname: None,
        };
        self.run(request, "Abwesend").await
    }

    pub async fn change_nickname(&self, name: &str) -> Result<(), String> {
        self.run(state(None, None, Some(name.to_owned())), "Name ändern")
            .await
    }

    pub async fn poke(&self, client_id: u16, text: &str) -> Result<(), String> {
        self.run(
            Message::Poke {
                client_id,
                text: text.to_owned(),
            },
            "Anstupsen",
        )
        .await
    }

    pub async fn kick(
        &self,
        client_id: u16,
        from_server: bool,
        reason: &str,
    ) -> Result<(), String> {
        let what = if from_server {
            "Vom Server werfen"
        } else {
            "Aus Kanal werfen"
        };
        self.run(
            Message::Kick {
                client_id,
                from_server,
                reason: reason.to_owned(),
            },
            what,
        )
        .await
    }

    /// `duration_seconds` 0 heisst: fuer immer.
    pub async fn ban(
        &self,
        client_id: u16,
        reason: &str,
        duration_seconds: u32,
    ) -> Result<(), String> {
        let request = Message::Ban {
            client_id,
            reason: reason.to_owned(),
            duration_seconds,
        };
        self.run(request, "Bannen").await
    }

    pub async fn move_client(&self, client_id: u16, channel_id: u64) -> Result<(), String> {
        self.run(
            Message::MoveClient {
                client_id,
                channel_id,
            },
            "Client verschieben",
        )
        .await
    }

    pub async fn create_channel(
        &self,
        name: &str,
        parent: u64,
        permanent: bool,
    ) -> Result<(), String> {
        let request = Message::CreateChannel {
            name: name.to_owned(),
            parent,
            permanent,
            password: None,
            max_clients: -1,
            topic: None,
        };
        self.run(request, "Kanal anlegen").await
    }

    /// Aendert einen Kanal (echo-30). `password` `None` laesst das Passwort
    /// unberuehrt, `Some` setzt es — leer entfernt es.
    pub async fn edit_channel(
        &self,
        channel_id: u64,
        name: &str,
        topic: Option<String>,
        password: Option<String>,
        needed_talk_power: i32,
    ) -> Result<(), String> {
        let request = Message::EditChannel {
            channel_id,
            name: name.to_owned(),
            topic,
            change_password: password.is_some(),
            password,
            needed_talk_power,
        };
        self.run(request, "Kanal ändern").await
    }

    pub async fn delete_channel(&self, channel_id: u64) -> Result<(), String> {
        self.run(Message::DeleteChannel { channel_id }, "Kanal löschen")
            .await
    }

    pub async fn set_role(&self, fingerprint: &str, role: Role) -> Result<(), String> {
        self.run(
            Message::SetRole {
                fingerprint: fingerprint.to_owned(),
                role,
            },
            "Rolle setzen",
        )
        .await
    }

    pub async fn decide_approval(&self, fingerprint: &str, allow: bool) -> Result<(), String> {
        let request = Message::DecideApproval {
            fingerprint: fingerprint.to_owned(),
            allow,
        };
        self.run(request, "Freigabe").await
    }

    pub async fn approvals(&self) -> Result<Vec<echo_protocol::ApprovalDto>, String> {
        match self.ask(Message::ListApprovals).await? {
            Message::ApprovalList { entries } => Ok(entries),
            answer => Err(refusal(answer)),
        }
    }

    // --- Dateien (echo-20) ----------------------------------------------------

    pub async fn list_files(&self, channel: u64, folder: &str) -> Result<FileListing, String> {
        match self
            .ask(Message::ListFiles {
                channel,
                folder: folder.to_owned(),
            })
            .await?
        {
            Message::FileList {
                channel,
                folder,
                entries,
                used,
                limit,
            } => Ok(FileListing {
                channel,
                folder,
                entries,
                used,
                limit,
            }),
            answer => Err(refusal(answer)),
        }
    }

    pub async fn create_folder(
        &self,
        channel: u64,
        folder: &str,
        name: &str,
    ) -> Result<(), String> {
        let request = Message::CreateFolder {
            channel,
            folder: folder.to_owned(),
            name: name.to_owned(),
        };
        self.run(request, "Ordner anlegen").await
    }

    pub async fn rename_file(&self, file_id: &str, name: &str) -> Result<(), String> {
        let request = Message::RenameFile {
            file_id: file_id.to_owned(),
            name: name.to_owned(),
        };
        self.run(request, "Umbenennen").await
    }

    pub async fn delete_file(&self, file_id: &str) -> Result<(), String> {
        self.run(
            Message::DeleteFile {
                file_id: file_id.to_owned(),
            },
            "Löschen",
        )
        .await
    }

    pub async fn move_file(&self, file_id: &str, channel: u64, folder: &str) -> Result<(), String> {
        let request = Message::MoveFile {
            file_id: file_id.to_owned(),
            channel,
            folder: folder.to_owned(),
        };
        self.run(request, "Verschieben").await
    }

    // --- Tafeln (echo-183) ----------------------------------------------------

    pub async fn list_boards(&self, channel: u64) -> Result<Vec<BoardDto>, String> {
        match self.ask(Message::ListBoards { channel }).await? {
            Message::BoardList { boards, .. } => Ok(boards),
            answer => Err(refusal(answer)),
        }
    }

    pub async fn create_board(&self, channel: u64, name: &str) -> Result<(), String> {
        self.run(
            Message::CreateBoard {
                channel,
                name: name.to_owned(),
            },
            "Tafel anlegen",
        )
        .await
    }

    pub async fn rename_board(&self, board_id: &str, name: &str) -> Result<(), String> {
        let request = Message::RenameBoard {
            board_id: board_id.to_owned(),
            name: name.to_owned(),
        };
        self.run(request, "Tafel umbenennen").await
    }

    pub async fn delete_board(&self, board_id: &str) -> Result<(), String> {
        self.run(
            Message::DeleteBoard {
                board_id: board_id.to_owned(),
            },
            "Tafel löschen",
        )
        .await
    }

    /// Oeffnet eine Tafel zum Ansehen; der Stand kommt als `BoardOpened` und
    /// danach als `BoardEdited`.
    pub async fn open_board(&self, board_id: &str) -> Result<(), String> {
        match self
            .ask(Message::OpenBoard {
                board_id: board_id.to_owned(),
            })
            .await?
        {
            Message::BoardSnapshot { .. } => Ok(()),
            answer => Err(refusal(answer)),
        }
    }

    pub fn close_board(&self, board_id: &str) {
        self.tell(Message::CloseBoard {
            board_id: board_id.to_owned(),
        });
    }

    pub async fn join_board(&self, board_id: &str, join: bool) -> Result<(), String> {
        let what = if join { "Mitmachen" } else { "Nur zusehen" };
        self.run(
            Message::JoinBoard {
                board_id: board_id.to_owned(),
                join,
            },
            what,
        )
        .await
    }

    /// Eigene Aenderungen; das Echo kommt als `BoardEdited` mit `own`. Lehnt
    /// der Server ab, gilt keine davon.
    pub async fn edit_board(&self, board_id: &str, changes: Vec<ShapeDto>) -> Result<(), String> {
        self.request(Message::EditBoard {
            board_id: board_id.to_owned(),
            changes,
        })
        .await
    }

    pub fn board_pointer(&self, board_id: &str, x: f32, y: f32, laser: bool) {
        self.tell(Message::BoardPointer {
            board_id: board_id.to_owned(),
            x,
            y,
            laser,
        });
    }
}

fn state(
    input_muted: Option<bool>,
    output_muted: Option<bool>,
    nickname: Option<String>,
) -> Message {
    Message::SetState {
        input_muted,
        output_muted,
        away: None,
        away_message: None,
        nickname,
    }
}

/// Der Grund einer Ablehnung, oder dass die Antwort nicht passt.
fn refusal(answer: Message) -> String {
    match answer {
        Message::Reply { ok: false, error } => error.unwrap_or_else(|| "Fehler.".into()),
        _ => UNEXPECTED.into(),
    }
}
