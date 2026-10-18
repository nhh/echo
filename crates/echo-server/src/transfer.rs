//! Die Datenverbindung (echo-20): eine eigene QUIC-Verbindung, die statt
//! `Open` ein `TransferHello` schickt und danach auf demselben Stream nur
//! noch Bytes traegt. Sie laeuft neben dem Steuerkanal, damit eine grosse
//! Datei Sprache und Chat nicht ausbremst.
//!
//! Wie `BulkCopy` im C#-Vorgaenger (echo-175): in grossen Bloecken, und
//! waehrend ein Block geschrieben wird, wird schon der naechste gelesen.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use echo_protocol::{Envelope, Message, write_message};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::state::ServerState;
use crate::transfers::{Direction, PendingTransfer};

/// So lange darf ein einzelner Leseschritt stocken.
const IDLE: Duration = Duration::from_secs(30);

/// Groesse eines Blocks.
const BLOCK_BYTES: usize = 1024 * 1024;

/// Bloecke, die schon gelesen auf das Netz warten duerfen.
const BLOCKS_AHEAD: usize = 2;

#[derive(Debug, thiserror::Error)]
enum TransferError {
    #[error("Die Übertragung stockte zu lange.")]
    Idle,
    #[error("Die Übertragung brach ab.")]
    Truncated,
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Read(#[from] quinn::ReadError),
    #[error("{0}")]
    Write(#[from] quinn::WriteError),
    #[error("{0}")]
    Protocol(#[from] echo_protocol::ProtocolError),
    #[error("{0}")]
    Refused(String),
}

impl From<tokio::time::error::Elapsed> for TransferError {
    fn from(_: tokio::time::error::Elapsed) -> Self {
        Self::Idle
    }
}

/// Fuehrt die Uebertragung aus, fuer die das Token ausgestellt wurde.
pub(crate) async fn run(
    state: &Arc<ServerState>,
    mut send: quinn::SendStream,
    mut recv: quinn::RecvStream,
    request_id: u32,
    token: &str,
) {
    let Some(transfer) = state.redeem(token) else {
        // Erst auflegen, wenn der Client den Grund gelesen hat (echo-102).
        let _ = reply(
            &mut send,
            request_id,
            Err("Die Eintrittskarte ist abgelaufen.".into()),
        )
        .await;
        let _ = send.finish();
        wait_for_hang_up(&mut recv).await;
        return;
    };
    let result = match transfer.direction {
        Direction::Upload => receive(state, &mut send, &mut recv, request_id, &transfer).await,
        Direction::Download { offset, .. } => {
            send_content(state, &mut send, &mut recv, request_id, &transfer, offset).await
        }
    };
    // Wer direkt liest, legt beim Springen auf; das gehoert nicht ins Log (echo-178).
    let streaming = matches!(
        transfer.direction,
        Direction::Download {
            streaming: true,
            ..
        }
    );
    if let Err(error) = result
        && !streaming
    {
        state.note_transfer_failed(&transfer, &error.to_string());
    }
}

async fn reply(
    send: &mut quinn::SendStream,
    request_id: u32,
    result: Result<(), String>,
) -> Result<(), TransferError> {
    let (ok, error) = match result {
        Ok(()) => (true, None),
        Err(error) => (false, Some(error)),
    };
    let envelope = Envelope {
        request_id,
        message: Message::Reply { ok, error },
    };
    write_message(send, &envelope).await?;
    Ok(())
}

/// Nimmt genau `size` Bytes an. Bricht die Verbindung vorher ab, bleibt nur
/// die halbe Datei im Temp-Ordner und wird geloescht.
async fn receive(
    state: &Arc<ServerState>,
    send: &mut quinn::SendStream,
    recv: &mut quinn::RecvStream,
    request_id: u32,
    transfer: &PendingTransfer,
) -> Result<(), TransferError> {
    reply(send, request_id, Ok(())).await?;
    let temp = state.new_temp_path();
    if let Err(error) = receive_into(recv, &temp, transfer.size).await {
        let _ = tokio::fs::remove_file(&temp).await;
        return Err(error);
    }

    let completing = Arc::clone(state);
    let finished = transfer.clone();
    let result = tokio::task::spawn_blocking(move || completing.complete_upload(&finished, &temp))
        .await
        .unwrap_or_else(|_| Err("Übernahme abgebrochen.".into()));
    reply(send, request_id, result.clone()).await?;
    wait_for_hang_up(recv).await;
    result.map_err(TransferError::Refused)
}

async fn receive_into(
    recv: &mut quinn::RecvStream,
    temp: &Path,
    size: u64,
) -> Result<(), TransferError> {
    let mut file = tokio::fs::File::create(temp).await?;
    // Der Platz wird vorab belegt: passt die Datei nicht mehr hin, scheitert
    // es sofort statt nach Gigabytes.
    file.set_len(size).await?;
    let mut received: u64 = 0;
    while received < size {
        let left = usize::try_from(size - received)
            .unwrap_or(BLOCK_BYTES)
            .min(BLOCK_BYTES);
        let chunk = tokio::time::timeout(IDLE, recv.read_chunk(left, true))
            .await??
            .ok_or(TransferError::Truncated)?;
        file.write_all(&chunk.bytes).await?;
        received += chunk.bytes.len() as u64;
    }
    file.flush().await?;
    file.sync_all().await?;
    Ok(())
}

/// Sendet ab `offset` bis zum Ende. Gelesen wird in einem Thread fuer
/// blockierende Arbeit, je Block, waehrend der vorige schon unterwegs ist.
async fn send_content(
    state: &Arc<ServerState>,
    send: &mut quinn::SendStream,
    recv: &mut quinn::RecvStream,
    request_id: u32,
    transfer: &PendingTransfer,
    offset: u64,
) -> Result<(), TransferError> {
    let opening = Arc::clone(state);
    let wanted = transfer.clone();
    let content = tokio::task::spawn_blocking(move || opening.open_content(&wanted))
        .await
        .unwrap_or_else(|_| Err("Lesen abgebrochen.".into()));
    let Some(mut content) = content.map_err(TransferError::Refused)? else {
        reply(send, request_id, Err("Der Inhalt fehlt.".into())).await?;
        let _ = send.finish();
        wait_for_hang_up(recv).await;
        return Ok(());
    };
    reply(send, request_id, Ok(())).await?;

    let (blocks, mut incoming) = mpsc::channel::<std::io::Result<Vec<u8>>>(BLOCKS_AHEAD);
    tokio::task::spawn_blocking(move || {
        let start = offset.min(content.len());
        if let Err(error) = content.seek(SeekFrom::Start(start)) {
            let _ = blocks.blocking_send(Err(error));
            return;
        }
        loop {
            let mut block = vec![0u8; BLOCK_BYTES];
            match content.read(&mut block) {
                Ok(0) => return,
                Ok(read) => {
                    block.truncate(read);
                    if blocks.blocking_send(Ok(block)).is_err() {
                        return;
                    }
                }
                Err(error) => {
                    let _ = blocks.blocking_send(Err(error));
                    return;
                }
            }
        }
    });
    while let Some(block) = incoming.recv().await {
        send.write_all(&block?).await?;
    }
    let _ = send.finish();
    wait_for_hang_up(recv).await;
    Ok(())
}

/// Erst wenn der Client seine Seite schliesst, darf der Server auflegen;
/// sein Verbindungsabbau wuerde sonst noch nicht gelesene Daten verwerfen
/// (echo-41).
async fn wait_for_hang_up(recv: &mut quinn::RecvStream) {
    let _ = tokio::time::timeout(IDLE, async {
        let mut rest = [0u8; 64];
        while let Ok(Some(_)) = recv.read(&mut rest).await {}
    })
    .await;
}
