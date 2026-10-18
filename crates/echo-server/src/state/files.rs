//! Dateibereich, Uebertragungen und Chatbilder (echo-20, echo-166, echo-182):
//! der Teil von `ServerState`, der grosse Inhalte bewegt. Was dauert
//! (Import, Umzug zwischen Archiven), laeuft ausserhalb der Sperre.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use echo_protocol::{ArchiveRef, ChatScope, FileEntryDto, ImageFormat, Message, Rights, Wire};
use echo_storage::{ArchivedImage, ContentReader, is_valid_id};

use super::{Inner, Refusal, ServerState, broadcast, publish, readable, route, storage_refusal};
use crate::files::{self, FileRecord, MAX_DEPTH, is_valid_folder, is_valid_name, new_id};
use crate::ticks;
use crate::transfers::{Direction, PendingImage, PendingTransfer, TransferRequest};

const GONE: &str = "Der Ort gibt es nicht mehr oder der Name ist inzwischen vergeben.";
const MEGABYTE: u64 = 1024 * 1024;

/// Ein neuer Upload aus `RequestUpload`.
#[derive(Debug, Clone)]
pub(crate) struct UploadRequest {
    pub(crate) channel: u64,
    pub(crate) folder: String,
    pub(crate) name: String,
    pub(crate) size: u64,
}

/// Ein neues Chatbild aus `RequestChatImage`.
#[derive(Debug, Clone)]
pub(crate) struct ImageRequest {
    pub(crate) scope: ChatScope,
    pub(crate) target: u16,
    pub(crate) size: u64,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl ServerState {
    // --- Dateibereich ---------------------------------------------------------

    /// Den Inhalt eines Ordners; die Zahlen sind die serverweiten.
    pub(crate) fn list_files(&self, channel: u64, folder: &str) -> Result<Message, Refusal> {
        let inner = self.inner.lock();
        if !inner.channels.exists(channel) {
            return Err("Kanal existiert nicht.".into());
        }
        if !is_valid_folder(folder) || !inner.files.folder_exists(channel, folder) {
            return Err("Ordner existiert nicht.".into());
        }
        let entries = inner
            .files
            .list(channel, folder)
            .into_iter()
            .map(|r| FileEntryDto {
                id: r.id.clone(),
                name: r.name.clone(),
                is_folder: r.is_folder,
                size: r.size,
                uploaded_by_name: r.uploaded_by_name.clone(),
                uploaded_at: r.uploaded_at,
            })
            .collect();
        Ok(Message::FileList {
            channel,
            folder: folder.to_owned(),
            entries,
            used: inner.files.used_bytes(),
            limit: inner.settings.max_file_bytes,
        })
    }

    pub(crate) fn create_folder(
        &self,
        by: &crate::client::Client,
        channel: u64,
        folder: &str,
        name: &str,
    ) -> Result<(), Refusal> {
        let mut inner = self.inner.lock();
        if !Rights::may_upload(by.role()) {
            return Err("Keine Berechtigung.".into());
        }
        if !inner.channels.exists(channel) {
            return Err("Kanal existiert nicht.".into());
        }
        let clean = name.trim();
        if !is_valid_name(clean) || !is_valid_folder(folder) {
            return Err("Ungültiger Name.".into());
        }
        if !inner.files.folder_exists(channel, folder) {
            return Err("Ordner existiert nicht.".into());
        }
        let path = if folder.is_empty() {
            clean.to_owned()
        } else {
            format!("{folder}/{clean}")
        };
        if !is_valid_folder(&path) {
            return Err("Zu tief verschachtelt.".into());
        }
        if inner.files.exists(channel, folder, clean) {
            return Err("Name ist schon vergeben.".into());
        }
        inner
            .files
            .create_folder(channel, folder, clean, by.fingerprint(), &by.name())
            .map_err(storage_refusal)?;
        notify_files_changed(&inner, channel);
        Ok(())
    }

    /// Benennt um. Steht in `name` ein `/`, ist es der neue Pfad im Kanal, und
    /// der Eintrag wandert dorthin (echo-130), wie `rename` im Dateisystem;
    /// "/Datei.pdf" ist die Wurzel.
    pub(crate) fn rename_file(
        &self,
        by: &crate::client::Client,
        file_id: &str,
        name: &str,
    ) -> Result<(), Refusal> {
        let mut inner = self.inner.lock();
        let record = locate(&inner, by, file_id)?;
        let (folder, leaf) = target(&record, name.trim());
        if !is_valid_name(&leaf) || !is_valid_folder(&folder) {
            return Err("Ungültiger Name.".into());
        }
        if !inner.files.folder_exists(record.channel, &folder) {
            return Err("Ordner existiert nicht.".into());
        }
        if record.is_folder && inside(&folder, &record) {
            return Err("Ein Ordner kann nicht in sich selbst liegen.".into());
        }
        if inner.files.depth_after_move(&record, &folder) > MAX_DEPTH {
            return Err("Zu tief verschachtelt.".into());
        }
        let same = folder == record.folder && leaf.to_lowercase() == record.name.to_lowercase();
        if !same && inner.files.exists(record.channel, &folder, &leaf) {
            return Err("Name ist schon vergeben.".into());
        }
        inner
            .files
            .move_to(&record, record.channel, &folder, &leaf)
            .map_err(storage_refusal)?;
        notify_files_changed(&inner, record.channel);
        Ok(())
    }

    /// Legt einen Eintrag in einen Ordner eines Kanals, auch eines anderen
    /// (echo-173); der Inhalt zieht im Hintergrund hinterher.
    pub(crate) fn move_file(
        self: &Arc<Self>,
        by: &crate::client::Client,
        file_id: &str,
        channel: u64,
        folder: &str,
    ) -> Result<(), Refusal> {
        let mut inner = self.inner.lock();
        let record = locate(&inner, by, file_id)?;
        if !inner.channels.exists(channel) {
            return Err("Kanal existiert nicht.".into());
        }
        if !is_valid_folder(folder) || !inner.files.folder_exists(channel, folder) {
            return Err("Ordner existiert nicht.".into());
        }
        let same_channel = channel == record.channel;
        if same_channel && folder == record.folder {
            return Ok(());
        }
        if same_channel && record.is_folder && inside(folder, &record) {
            return Err("Ein Ordner kann nicht in sich selbst liegen.".into());
        }
        if inner.files.depth_after_move(&record, folder) > MAX_DEPTH {
            return Err("Zu tief verschachtelt.".into());
        }
        if inner.files.exists(channel, folder, &record.name) {
            return Err("Name ist schon vergeben.".into());
        }

        inner
            .files
            .move_to(&record, channel, folder, &record.name)
            .map_err(storage_refusal)?;
        if !same_channel {
            self.relocate_later(&mut inner);
            inner.db.log_event(
                "file",
                &format!(
                    "{} verschiebt '{}' von Kanal {} nach Kanal {channel}",
                    by.name(),
                    record.full_path(),
                    record.channel
                ),
            );
            notify_files_changed(&inner, record.channel);
        }
        notify_files_changed(&inner, channel);
        Ok(())
    }

    pub(crate) fn delete_file(
        &self,
        by: &crate::client::Client,
        file_id: &str,
    ) -> Result<(), Refusal> {
        let mut inner = self.inner.lock();
        let record = locate(&inner, by, file_id)?;
        inner.files.delete(&record).map_err(storage_refusal)?;
        inner.db.log_event(
            "file",
            &format!(
                "{} löscht '{}' in Kanal {}",
                by.name(),
                record.full_path(),
                record.channel
            ),
        );
        notify_files_changed(&inner, record.channel);
        Ok(())
    }

    // --- Uebertragung (echo-20) -----------------------------------------------

    /// Platz fuer eine Datei: prueft Recht, Ort, Name und die serverweite
    /// Obergrenze und gibt eine Eintrittskarte aus. Eine Grenze je Datei gibt
    /// es nicht (echo-174).
    pub(crate) fn begin_upload(
        &self,
        by: &crate::client::Client,
        request: UploadRequest,
    ) -> Result<Message, Refusal> {
        let mut inner = self.inner.lock();
        if !Rights::may_upload(by.role()) {
            return Err("Keine Berechtigung.".into());
        }
        if !inner.channels.exists(request.channel) {
            return Err("Kanal existiert nicht.".into());
        }
        let name = request.name.trim().to_owned();
        if !is_valid_name(&name) || !is_valid_folder(&request.folder) {
            return Err("Ungültiger Name.".into());
        }
        if !inner.files.folder_exists(request.channel, &request.folder) {
            return Err("Ordner existiert nicht.".into());
        }
        if inner.files.exists(request.channel, &request.folder, &name) {
            return Err("Name ist schon vergeben.".into());
        }
        if !inner
            .files
            .has_room_for(request.size, inner.settings.max_file_bytes)
        {
            return Err("Auf dem Server ist kein Platz mehr.".into());
        }
        let transfer = issue(
            &mut inner,
            TransferRequest {
                direction: Direction::Upload,
                channel: request.channel,
                folder: request.folder,
                name,
                size: request.size,
                file_id: None,
                by: by.fingerprint().to_owned(),
                by_name: by.name(),
                image: None,
            },
        )?;
        Ok(ticket(&transfer))
    }

    /// `offset` ist, wo die Datei beginnen soll; `streaming`, ob Kino, Radio
    /// oder Bildbetrachter sie direkt lesen (echo-178).
    pub(crate) fn begin_download(
        &self,
        by: &crate::client::Client,
        file_id: &str,
        offset: u64,
        streaming: bool,
    ) -> Result<Message, Refusal> {
        let mut inner = self.inner.lock();
        let record = inner
            .files
            .find(file_id)
            .filter(|r| !r.is_folder)
            .cloned()
            .ok_or("Der Eintrag existiert nicht mehr.")?;
        if !inner.files.has_content(&record) {
            return Err("Der Inhalt fehlt.".into());
        }
        if offset > record.size {
            return Err("Diese Stelle gibt es in der Datei nicht.".into());
        }
        let transfer = issue(
            &mut inner,
            TransferRequest {
                direction: Direction::Download { offset, streaming },
                channel: record.channel,
                folder: record.folder,
                name: record.name,
                size: record.size,
                file_id: Some(record.id),
                by: by.fingerprint().to_owned(),
                by_name: by.name(),
                image: None,
            },
        )?;
        Ok(ticket(&transfer))
    }

    /// Loest ein Token der Datenverbindung ein.
    pub(crate) fn redeem(&self, token: &str) -> Option<PendingTransfer> {
        self.inner.lock().transfers.redeem(token)
    }

    /// Wohin ein Upload schreibt, bevor er uebernommen wird.
    pub(crate) fn new_temp_path(&self) -> PathBuf {
        self.inner.lock().files.new_temp_path()
    }

    /// Der fertige Upload wird Teil des Kanals. Passt er nicht mehr (der Ort
    /// ist weg, der Name inzwischen vergeben), wird er verworfen. Blockiert:
    /// bei einem Film dauert das Uebernehmen ins Archiv.
    pub(crate) fn complete_upload(
        &self,
        transfer: &PendingTransfer,
        temp: &Path,
    ) -> Result<(), Refusal> {
        if let Some(image) = &transfer.image {
            let result = self.complete_chat_image(transfer, image, temp);
            let _ = std::fs::remove_file(temp);
            return result;
        }

        let id = new_id();
        let archives = {
            let inner = self.inner.lock();
            if !fits(&inner, transfer) {
                let _ = std::fs::remove_file(temp);
                return Err(GONE.into());
            }
            Arc::clone(&inner.archives)
        };
        // Ins Archiv des Kanals, ausserhalb der Sperre.
        let imported = files::import(&archives, &id, transfer.channel, temp);
        let _ = std::fs::remove_file(temp);
        let size = imported.map_err(|_| GONE.to_owned())?;

        let mut inner = self.inner.lock();
        if !fits(&inner, transfer) {
            inner.files.drop_content(transfer.channel, &id);
            return Err(GONE.into());
        }
        let record = FileRecord {
            id,
            channel: transfer.channel,
            folder: transfer.folder.clone(),
            name: transfer.name.clone(),
            is_folder: false,
            size,
            uploaded_by: transfer.by.clone(),
            uploaded_by_name: transfer.by_name.clone(),
            uploaded_at: ticks::unix_millis_now(),
            content_channel: transfer.channel,
        };
        let path = record.full_path();
        inner.files.add(record).map_err(storage_refusal)?;
        inner.db.log_event(
            "file",
            &format!(
                "{} legt '{path}' in Kanal {} ab",
                transfer.by_name, transfer.channel
            ),
        );
        notify_files_changed(&inner, transfer.channel);
        Ok(())
    }

    /// Ein Lesestrom fuer einen Download: das Bild aus seinem Archiv oder die
    /// Datei aus dem Dateibereich; `None`, wenn der Inhalt fehlt.
    pub(crate) fn open_content(
        &self,
        transfer: &PendingTransfer,
    ) -> Result<Option<ContentReader>, Refusal> {
        let inner = self.inner.lock();
        if let Some(image) = &transfer.image {
            let store = match image.scope {
                ChatScope::Server => Some(Arc::clone(inner.archives.server())),
                ChatScope::Channel if inner.channels.exists(image.channel) => inner
                    .archives
                    .existing_channel(image.channel)
                    .map_err(storage_refusal)?,
                ChatScope::Channel => None,
                ChatScope::Private => inner
                    .archives
                    .dm(&transfer.by, &image.peer, false)
                    .map_err(storage_refusal)?,
            };
            return match store {
                Some(store) => store.open_image(&image.id).map_err(storage_refusal),
                None => Ok(None),
            };
        }
        let Some(record) = transfer
            .file_id
            .as_deref()
            .and_then(|id| inner.files.find(id))
        else {
            return Ok(None);
        };
        inner.files.open_content(record).map_err(storage_refusal)
    }

    /// Haelt fest, dass eine Uebertragung nicht zu Ende lief: im Ereignislog
    /// und in der Ausgabe, damit man es im Journal sieht.
    pub(crate) fn note_transfer_failed(&self, transfer: &PendingTransfer, reason: &str) {
        let what = if transfer.direction == Direction::Upload {
            "Upload"
        } else {
            "Download"
        };
        let line = format!(
            "{what} von '{}' durch {} abgebrochen: {reason}",
            transfer.name, transfer.by_name
        );
        self.inner.lock().db.log_event("file", &line);
        self.log(&line);
    }

    /// Nach dem Verschieben in einen anderen Kanal ziehen die Inhalte
    /// hinterher (echo-182): kopieren im Hintergrund, dann umhaengen. Bis dahin
    /// liest ein Download aus dem alten Archiv.
    fn relocate_later(self: &Arc<Self>, inner: &mut Inner) {
        let fresh: Vec<_> = inner
            .files
            .relocating()
            .into_iter()
            .filter(|r| inner.relocating.insert(r.id.clone()))
            .collect();
        if fresh.is_empty() {
            return;
        }
        let state = Arc::clone(self);
        let archives = Arc::clone(&inner.archives);
        tokio::task::spawn_blocking(move || state.relocate(&archives, fresh));
    }

    fn relocate(
        self: &Arc<Self>,
        archives: &crate::archives::ArchiveSet,
        records: Vec<FileRecord>,
    ) {
        let mut failed = Vec::new();
        for record in records {
            match files::copy_content(archives, &record) {
                Ok(true) => {
                    if let Err(error) = self.inner.lock().files.finish_relocation(&record) {
                        self.log(&format!(
                            "Inhalt von '{}' zieht nicht um: {error}",
                            record.name
                        ));
                    }
                }
                Ok(false) => failed.push(record.id.clone()),
                Err(error) => {
                    self.log(&format!(
                        "Inhalt von '{}' zieht nicht um: {error}",
                        record.name
                    ));
                    failed.push(record.id.clone());
                }
            }
            self.inner.lock().relocating.remove(&record.id);
        }

        // Wurde waehrenddessen weiter verschoben, zieht es jetzt hinterher; was
        // nicht zu kopieren war, bleibt, sonst liefe das endlos.
        let mut inner = self.inner.lock();
        let again: Vec<_> = inner
            .files
            .relocating()
            .into_iter()
            .filter(|r| !failed.contains(&r.id) && inner.relocating.insert(r.id.clone()))
            .collect();
        if !again.is_empty() {
            let state = Arc::clone(self);
            let archives = Arc::clone(&inner.archives);
            tokio::task::spawn_blocking(move || state.relocate(&archives, again));
        }
    }

    // --- Chatbilder (echo-166) ------------------------------------------------

    /// Platz fuer ein Chatbild. Das Ziel wird hier festgehalten: ein Bild fuer
    /// den Kanal geht an den Kanal, in dem der Absender beim Senden war.
    pub(crate) fn begin_chat_image(
        &self,
        by: &crate::client::Client,
        request: &ImageRequest,
    ) -> Result<Message, Refusal> {
        let mut inner = self.inner.lock();
        if !Rights::may_send_images(by.role()) {
            return Err("Keine Berechtigung.".into());
        }
        if request.size == 0 || request.size > Wire::MAX_CHAT_IMAGE_BYTES {
            return Err(format!(
                "Bilder dürfen höchstens {} MB groß sein.",
                Wire::MAX_CHAT_IMAGE_BYTES / MEGABYTE
            ));
        }
        let side_ok = |side: u32| (1..=Wire::MAX_CHAT_IMAGE_SIDE).contains(&side);
        if !side_ok(request.width) || !side_ok(request.height) {
            return Err("Das Bild ist zu groß.".into());
        }
        if request.scope == ChatScope::Private && inner.clients.get(request.target).is_none() {
            return Err("Der Empfänger ist nicht mehr online.".into());
        }

        let image = PendingImage {
            id: new_id(),
            scope: request.scope,
            channel: by.channel(),
            target: request.target,
            from: by.id(),
            width: request.width,
            height: request.height,
            peer: String::new(),
        };
        let transfer = issue(
            &mut inner,
            TransferRequest {
                direction: Direction::Upload,
                channel: by.channel(),
                folder: String::new(),
                name: image.id.clone(),
                size: request.size,
                file_id: None,
                by: by.fingerprint().to_owned(),
                by_name: by.name(),
                image: Some(image),
            },
        )?;
        Ok(ticket(&transfer))
    }

    pub(crate) fn begin_image_download(
        &self,
        by: &crate::client::Client,
        archive: &ArchiveRef,
        image_id: &str,
    ) -> Result<Message, Refusal> {
        let mut inner = self.inner.lock();
        let store = readable(&inner, by, archive)?;
        let image = store
            .filter(|_| is_valid_id(image_id))
            .and_then(|store| store.image(image_id).ok().flatten())
            .ok_or("Das Bild gibt es in diesem Verlauf nicht.")?;
        let pending = PendingImage {
            id: image_id.to_owned(),
            scope: archive.scope,
            channel: archive.channel,
            target: 0,
            from: by.id(),
            width: u32::try_from(image.width).unwrap_or(0),
            height: u32::try_from(image.height).unwrap_or(0),
            peer: archive.peer.clone(),
        };
        let transfer = issue(
            &mut inner,
            TransferRequest {
                direction: Direction::Download {
                    offset: 0,
                    streaming: false,
                },
                channel: archive.channel,
                folder: String::new(),
                name: image_id.to_owned(),
                size: image.size,
                file_id: Some(image_id.to_owned()),
                by: by.fingerprint().to_owned(),
                by_name: by.name(),
                image: Some(pending),
            },
        )?;
        Ok(ticket(&transfer))
    }

    /// Das Bild ist da: pruefen, in das Archiv seiner Nachricht legen und als
    /// Nachricht verteilen. Bis 16 MB dauert das Ablegen Millisekunden; die
    /// Sperre haelt das aus.
    fn complete_chat_image(
        &self,
        transfer: &PendingTransfer,
        image: &PendingImage,
        temp: &Path,
    ) -> Result<(), Refusal> {
        if sniff(temp).is_none() {
            return Err("Das ist kein Bild (erlaubt sind PNG, JPEG, GIF, WebP und BMP).".into());
        }
        let inner = self.inner.lock();
        let from = inner
            .clients
            .get(image.from)
            .filter(|c| c.fingerprint() == transfer.by)
            .ok_or("Der Absender ist nicht mehr verbunden.")?;
        let route = route(&inner, &from, image.scope, image.channel, image.target)
            .ok_or("Der Empfänger ist nicht mehr online.")?;
        let archived = ArchivedImage {
            id: image.id.clone(),
            width: i64::from(image.width),
            height: i64::from(image.height),
            size: transfer.size,
        };
        let mut content = std::fs::File::open(temp).map_err(|e| e.to_string())?;
        route
            .archive
            .put_image(&archived, &mut content)
            .map_err(storage_refusal)?;
        publish(&route, &from, "", Some(&archived.id)).map_err(storage_refusal)?;
        inner.db.log_event(
            "image",
            &format!(
                "{} schickt ein Bild ({} Byte) an {:?}",
                from.name(),
                transfer.size,
                image.scope
            ),
        );
        Ok(())
    }
}

/// Findet den Eintrag und prueft, ob dieser Client ihn aendern darf: die
/// eigenen Sachen jeder, der hochladen darf, fremde nur die Moderation.
fn locate(inner: &Inner, by: &crate::client::Client, file_id: &str) -> Result<FileRecord, Refusal> {
    let record = inner
        .files
        .find(file_id)
        .cloned()
        .ok_or("Der Eintrag existiert nicht mehr.")?;
    let own = record.uploaded_by == by.fingerprint();
    let allowed = if own {
        Rights::may_upload(by.role())
    } else {
        Rights::may_delete_foreign_files(by.role())
    };
    if allowed {
        Ok(record)
    } else {
        Err("Keine Berechtigung.".into())
    }
}

/// Ordner und Name, wohin `RenameFile` einen Eintrag bringt.
fn target(record: &FileRecord, name: &str) -> (String, String) {
    if !name.contains('/') {
        return (record.folder.clone(), name.to_owned());
    }
    let path = name.strip_prefix('/').unwrap_or(name);
    match path.rsplit_once('/') {
        Some((folder, leaf)) => (folder.to_owned(), leaf.to_owned()),
        None => (String::new(), path.to_owned()),
    }
}

/// Liegt `folder` im Ordner `record` oder ist er es selbst?
fn inside(folder: &str, record: &FileRecord) -> bool {
    let path = record.full_path();
    folder == path || folder.starts_with(&format!("{path}/"))
}

/// Gibt es den Ort noch, und ist der Name noch frei? Unter der Sperre.
fn fits(inner: &Inner, transfer: &PendingTransfer) -> bool {
    inner.channels.exists(transfer.channel)
        && inner
            .files
            .folder_exists(transfer.channel, &transfer.folder)
        && !inner
            .files
            .exists(transfer.channel, &transfer.folder, &transfer.name)
}

fn issue(inner: &mut Inner, request: TransferRequest) -> Result<PendingTransfer, Refusal> {
    let token = echo_protocol::new_nonce().map_err(|e| e.to_string())?;
    Ok(inner.transfers.issue(token, request))
}

fn ticket(transfer: &PendingTransfer) -> Message {
    Message::TransferTicket {
        token: transfer.token.clone(),
        name: transfer.name.clone(),
        size: transfer.size,
    }
}

fn notify_files_changed(inner: &Inner, channel: u64) {
    broadcast(inner, &Message::FilesChanged { channel }, None);
}

fn sniff(path: &Path) -> Option<ImageFormat> {
    use std::io::Read;
    let mut head = [0u8; ImageFormat::HEADER_BYTES];
    let mut file = std::fs::File::open(path).ok()?;
    let mut read = 0;
    while read < head.len() {
        match file.read(&mut head[read..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => read += n,
        }
    }
    ImageFormat::sniff(&head[..read])
}
