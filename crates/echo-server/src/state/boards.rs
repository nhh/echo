//! Tafeln (echo-183): wer im Kanal ist, sieht eine offene Tafel mit; wer
//! mitmacht, zeichnet. Formen liegen als JSON im Archiv des Kanals, der
//! Server ordnet die Aenderungen, die letzte gewinnt.

use std::sync::Arc;

use echo_protocol::{BoardDto, Message, Rights, ShapeDto};
use echo_storage::{ArchivedBoard, ChannelArchive, is_valid_id};

use super::{Inner, Refusal, ServerState, storage_refusal};
use crate::client::Client;
use crate::files::new_id;
use crate::ticks;

const MAX_BOARDS_PER_CHANNEL: usize = 50;
const MAX_BOARD_NAME: usize = 60;
const MAX_CHANGES_PER_EDIT: usize = 500;

/// Die Formen beim Oeffnen gehen in Portionen dieser Groesse hinaus; eine
/// volle Tafel ist groesser als eine Nachricht.
const SNAPSHOT_BATCH_BYTES: usize = 256 * 1024;

/// So lang darf eine Form als JSON sein; der Server kennt die Formen nicht.
const MAX_SHAPE_JSON: usize = 64 * 1024;

impl ServerState {
    pub(crate) fn list_boards(&self, by: &Client, channel: u64) -> Result<Message, Refusal> {
        let inner = self.inner.lock();
        in_channel(by, channel)?;
        let boards = inner
            .archives
            .channel(channel)
            .and_then(|a| a.boards())
            .map_err(storage_refusal)?
            .into_iter()
            .map(|b| BoardDto {
                id: b.id,
                name: b.name,
            })
            .collect();
        Ok(Message::BoardList { channel, boards })
    }

    pub(crate) fn create_board(
        &self,
        by: &Client,
        channel: u64,
        name: &str,
    ) -> Result<(), Refusal> {
        let mut inner = self.inner.lock();
        in_channel(by, channel)?;
        if !Rights::may_upload(by.role()) {
            return Err("Keine Berechtigung.".into());
        }
        let clean = valid_name(name)?;
        let archive = inner.archives.channel(channel).map_err(storage_refusal)?;
        if archive.boards().map_err(storage_refusal)?.len() >= MAX_BOARDS_PER_CHANNEL {
            return Err(format!(
                "Ein Kanal hat höchstens {MAX_BOARDS_PER_CHANNEL} Tafeln."
            ));
        }
        let id = new_id();
        archive
            .save_board(&ArchivedBoard {
                id: id.clone(),
                name: clean,
                created_by: by.fingerprint().to_owned(),
                created_at: ticks::unix_millis_now(),
            })
            .map_err(storage_refusal)?;
        inner.boards.add(&id, channel);
        notify_boards_changed(&inner, channel);
        Ok(())
    }

    pub(crate) fn rename_board(
        &self,
        by: &Client,
        board_id: &str,
        name: &str,
    ) -> Result<(), Refusal> {
        let inner = self.inner.lock();
        let (board, archive, channel) = find_board(&inner, by, board_id, true)?;
        let name = valid_name(name)?;
        archive
            .save_board(&ArchivedBoard { name, ..board })
            .map_err(storage_refusal)?;
        notify_boards_changed(&inner, channel);
        Ok(())
    }

    pub(crate) fn delete_board(&self, by: &Client, board_id: &str) -> Result<(), Refusal> {
        let mut inner = self.inner.lock();
        let (_, archive, channel) = find_board(&inner, by, board_id, true)?;
        archive.delete_board(board_id).map_err(storage_refusal)?;
        inner.boards.remove(board_id);
        inner.db.log_event(
            "board",
            &format!("{} löscht eine Tafel in Kanal {channel}", by.name()),
        );
        notify_boards_changed(&inner, channel);
        Ok(())
    }

    /// Ansehen darf, wer im Kanal ist. Nach der Antwort kommen die Formen in
    /// Portionen, noch unter der Sperre, damit keine Aenderung dazwischen
    /// faellt, die schon im Stand steckt.
    pub(crate) fn open_board(
        &self,
        by: &Client,
        board_id: &str,
        request_id: u32,
    ) -> Result<(), Refusal> {
        let mut inner = self.inner.lock();
        let (board, archive, _) = find_board(&inner, by, board_id, false)?;
        inner.boards.view(board_id, by.id());
        by.reply(
            request_id,
            Message::BoardSnapshot {
                board_id: board_id.to_owned(),
                name: board.name,
                participants: inner.boards.participants(board_id),
            },
        );

        let mut batch = Vec::new();
        let mut bytes = 0;
        for shape in archive.board_shapes(board_id).map_err(storage_refusal)? {
            if bytes + shape.json.len() > SNAPSHOT_BATCH_BYTES && !batch.is_empty() {
                send_batch(by, board_id, std::mem::take(&mut batch));
                bytes = 0;
            }
            bytes += shape.json.len();
            batch.push(ShapeDto {
                id: shape.id,
                json: Some(shape.json),
            });
        }
        if !batch.is_empty() {
            send_batch(by, board_id, batch);
        }
        Ok(())
    }

    pub(crate) fn close_board(&self, by: &Client, board_id: &str) {
        let mut inner = self.inner.lock();
        if inner.boards.leave(board_id, by.id()) {
            broadcast_presence(&inner, board_id);
        }
    }

    /// Mitmachen oder wieder nur zusehen; nur, wer die Tafel ansieht.
    pub(crate) fn join_board(
        &self,
        by: &Client,
        board_id: &str,
        join: bool,
    ) -> Result<(), Refusal> {
        let mut inner = self.inner.lock();
        if !inner.boards.is_viewing(board_id, by.id()) {
            return Err("Die Tafel ist nicht offen.".into());
        }
        if inner.boards.participate(board_id, by.id(), join) {
            broadcast_presence(&inner, board_id);
        }
        Ok(())
    }

    /// Aenderungen eines Mitmachenden: ablegen, dann an alle, die zusehen,
    /// auch an ihn selbst — in dieser Reihenfolge gewinnt die letzte.
    pub(crate) fn edit_board(
        &self,
        by: &Client,
        board_id: &str,
        changes: Vec<ShapeDto>,
    ) -> Result<(), Refusal> {
        let inner = self.inner.lock();
        if !inner.boards.is_participant(board_id, by.id()) {
            return Err("Erst mitmachen, dann zeichnen.".into());
        }
        let invalid = changes.len() > MAX_CHANGES_PER_EDIT
            || changes.iter().any(|c| {
                !is_valid_id(&c.id) || c.json.as_ref().is_some_and(|j| j.len() > MAX_SHAPE_JSON)
            });
        if invalid {
            return Err("Ungültige Änderung.".into());
        }
        let channel = inner
            .boards
            .channel_of(board_id)
            .ok_or("Die Tafel gibt es nicht mehr.")?;
        let stored: Vec<_> = changes
            .iter()
            .map(|c| (c.id.clone(), c.json.clone()))
            .collect();
        inner
            .archives
            .channel(channel)
            .and_then(|a| a.change_board(board_id, &stored))
            .map_err(storage_refusal)?;
        let edited = Message::BoardEdited {
            board_id: board_id.to_owned(),
            by: by.id(),
            changes,
        };
        for viewer in viewers(&inner, board_id) {
            viewer.send(edited.clone());
        }
        Ok(())
    }

    /// Der Zeiger eines Mitmachenden, an alle anderen, die zusehen. Nicht
    /// gespeichert; wer nicht mitmacht, zeigt keinen.
    pub(crate) fn move_pointer(&self, by: &Client, board_id: &str, x: f32, y: f32, laser: bool) {
        let inner = self.inner.lock();
        if !inner.boards.is_participant(board_id, by.id()) || !x.is_finite() || !y.is_finite() {
            return;
        }
        let moved = Message::BoardPointerMoved {
            board_id: board_id.to_owned(),
            by: by.id(),
            x,
            y,
            laser,
        };
        for viewer in viewers(&inner, board_id) {
            if viewer.id() != by.id() {
                viewer.send(moved.clone());
            }
        }
    }
}

/// Wer den Kanal verlaesst, sieht dessen Tafeln nicht mehr. Unter der Sperre.
pub(super) fn leave_boards(inner: &mut Inner, client: u16, keep_channel: Option<u64>) {
    for id in inner.boards.viewed_by(client, keep_channel) {
        if inner.boards.leave(&id, client) {
            broadcast_presence(inner, &id);
        }
    }
}

/// Nur im eigenen Kanal.
fn in_channel(by: &Client, channel: u64) -> Result<(), Refusal> {
    if by.channel() == channel {
        Ok(())
    } else {
        Err("Die Tafeln eines Kanals sieht nur, wer darin ist.".into())
    }
}

fn valid_name(name: &str) -> Result<String, Refusal> {
    let clean = name.trim();
    if (1..=MAX_BOARD_NAME).contains(&clean.chars().count()) {
        Ok(clean.to_owned())
    } else {
        Err("Ungültiger Name.".into())
    }
}

/// Findet eine Tafel, die `by` sehen darf; `may_change` verlangt dazu das
/// Recht, sie zu verwalten: die eigenen, wer hochladen darf, fremde nur die
/// Moderation.
fn find_board(
    inner: &Inner,
    by: &Client,
    board_id: &str,
    may_change: bool,
) -> Result<(ArchivedBoard, Arc<ChannelArchive>, u64), Refusal> {
    const GONE: &str = "Die Tafel gibt es nicht mehr.";
    let channel = inner.boards.channel_of(board_id).ok_or(GONE)?;
    in_channel(by, channel)?;
    let archive = inner.archives.channel(channel).map_err(storage_refusal)?;
    let board = archive
        .boards()
        .map_err(storage_refusal)?
        .into_iter()
        .find(|b| b.id == board_id)
        .ok_or(GONE)?;
    let allowed = if board.created_by == by.fingerprint() {
        Rights::may_upload(by.role())
    } else {
        Rights::may_delete_foreign_files(by.role())
    };
    if may_change && !allowed {
        return Err("Keine Berechtigung.".into());
    }
    Ok((board, archive, channel))
}

fn viewers(inner: &Inner, board_id: &str) -> Vec<Arc<Client>> {
    inner
        .boards
        .viewers(board_id)
        .into_iter()
        .filter_map(|id| inner.clients.get(id))
        .collect()
}

fn send_batch(to: &Client, board_id: &str, changes: Vec<ShapeDto>) {
    to.send(Message::BoardEdited {
        board_id: board_id.to_owned(),
        by: 0,
        changes,
    });
}

fn notify_boards_changed(inner: &Inner, channel: u64) {
    let changed = Message::BoardsChanged { channel };
    for member in inner.clients.in_channel(channel) {
        member.send(changed.clone());
    }
}

fn broadcast_presence(inner: &Inner, board_id: &str) {
    let presence = Message::BoardPresence {
        board_id: board_id.to_owned(),
        participants: inner.boards.participants(board_id),
    };
    for viewer in viewers(inner, board_id) {
        viewer.send(presence.clone());
    }
}
