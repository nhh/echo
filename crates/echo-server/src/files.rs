//! Der Dateibereich aller Kanaele (echo-20). Die Eintraege stehen in den
//! Archiven ihrer Kanaele (Tabelle `files`), die Inhalte dort als
//! sqlar-Eintraege (echo-182); hier liegt nur der Ueberblick im Speicher.
//! Ordner gibt es nur als Eintraege.
//!
//! Alles unter der Sperre von `ServerState`, ausser [`FileStore::import`] und
//! [`FileStore::copy_content`]: die bewegen grosse Inhalte und laufen
//! ausserhalb, ueber ihre eigenen Verbindungen zum Archiv.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use echo_storage::{ArchivedFile, ContentReader, StorageError};

use crate::archives::ArchiveSet;
use crate::ticks;

pub(crate) const MAX_NAME_LENGTH: usize = 120;
pub(crate) const MAX_DEPTH: usize = 8;
const PARTIAL_EXTENSION: &str = "part";
const ID_BYTES: usize = 16;

/// Ein Ordner oder eine Datei. `id` bleibt ueber Umbenennen und Verschieben
/// stabil. Der Inhalt liegt im Archiv des Kanals `content_channel`: nach dem
/// Verschieben in einen anderen Kanal noch im alten, bis er umgezogen ist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileRecord {
    pub(crate) id: String,
    pub(crate) channel: u64,
    pub(crate) folder: String,
    pub(crate) name: String,
    pub(crate) is_folder: bool,
    pub(crate) size: u64,
    pub(crate) uploaded_by: String,
    pub(crate) uploaded_by_name: String,
    /// Unix-Millisekunden.
    pub(crate) uploaded_at: i64,
    pub(crate) content_channel: u64,
}

impl FileRecord {
    /// Der Pfad des Eintrags im Kanal.
    pub(crate) fn full_path(&self) -> String {
        if self.folder.is_empty() {
            self.name.clone()
        } else {
            format!("{}/{}", self.folder, self.name)
        }
    }
}

#[derive(Debug)]
pub(crate) struct FileStore {
    archives: Arc<ArchiveSet>,
    incoming: PathBuf,
    records: HashMap<String, FileRecord>,
}

impl FileStore {
    /// Liest die Eintraege aus den Archiven der Kanaele und raeumt auf, was
    /// abgebrochene Uploads liegen liessen: das wird nie mehr fertig.
    pub(crate) fn load(
        archives: Arc<ArchiveSet>,
        incoming: PathBuf,
        channels: impl IntoIterator<Item = u64>,
    ) -> Result<Self, StorageError> {
        std::fs::create_dir_all(&incoming)?;
        for entry in std::fs::read_dir(&incoming)?.filter_map(Result::ok) {
            if entry
                .path()
                .extension()
                .is_some_and(|e| e == PARTIAL_EXTENSION)
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }

        let mut records = HashMap::new();
        for channel in channels {
            for file in archives.channel(channel)?.files()? {
                records.insert(file.id.clone(), from_archived(file, channel));
            }
        }
        Ok(Self {
            archives,
            incoming,
            records,
        })
    }

    /// Ein frischer Pfad fuer einen laufenden Upload.
    pub(crate) fn new_temp_path(&self) -> PathBuf {
        self.incoming
            .join(format!("upload-{}.{PARTIAL_EXTENSION}", new_id()))
    }

    // --- Lesen ----------------------------------------------------------------

    /// Ordner zuerst, dann nach Namen ohne Gross- und Kleinschreibung.
    pub(crate) fn list(&self, channel: u64, folder: &str) -> Vec<&FileRecord> {
        let mut list: Vec<_> = self
            .records
            .values()
            .filter(|r| r.channel == channel && r.folder == folder)
            .collect();
        list.sort_by_key(|r| (!r.is_folder, r.name.to_lowercase()));
        list
    }

    pub(crate) fn find(&self, id: &str) -> Option<&FileRecord> {
        self.records.get(id)
    }

    /// Belegt auf dem ganzen Server; begrenzt wird nur serverweit.
    pub(crate) fn used_bytes(&self) -> u64 {
        self.records
            .values()
            .filter(|r| !r.is_folder)
            .map(|r| r.size)
            .sum()
    }

    pub(crate) fn has_room_for(&self, size: u64, limit: u64) -> bool {
        self.used_bytes().saturating_add(size) <= limit
    }

    pub(crate) fn exists(&self, channel: u64, folder: &str, name: &str) -> bool {
        self.records.values().any(|r| {
            r.channel == channel
                && r.folder == folder
                && r.name.to_lowercase() == name.to_lowercase()
        })
    }

    /// Ein Ordner muss es geben, bevor etwas hineingelegt wird; die Wurzel
    /// gibt es immer.
    pub(crate) fn folder_exists(&self, channel: u64, folder: &str) -> bool {
        if folder.is_empty() {
            return true;
        }
        let (parent, name) = folder.rsplit_once('/').unwrap_or(("", folder));
        self.records.values().any(|r| {
            r.is_folder
                && r.channel == channel
                && r.folder == parent
                && r.name.to_lowercase() == name.to_lowercase()
        })
    }

    pub(crate) fn has_content(&self, record: &FileRecord) -> bool {
        !record.is_folder
            && self
                .archives
                .existing_channel(record.content_channel)
                .ok()
                .flatten()
                .and_then(|archive| archive.file_length(&record.id).ok().flatten())
                .is_some()
    }

    /// Ein Lesestrom auf den Inhalt, an jede Stelle setzbar; `None`, wenn er fehlt.
    pub(crate) fn open_content(
        &self,
        record: &FileRecord,
    ) -> Result<Option<ContentReader>, StorageError> {
        if record.is_folder {
            return Ok(None);
        }
        match self.archives.existing_channel(record.content_channel)? {
            Some(archive) => archive.open_file(&record.id),
            None => Ok(None),
        }
    }

    // --- Schreiben ------------------------------------------------------------

    pub(crate) fn create_folder(
        &mut self,
        channel: u64,
        folder: &str,
        name: &str,
        by: &str,
        by_name: &str,
    ) -> Result<(), StorageError> {
        self.save(FileRecord {
            id: new_id(),
            channel,
            folder: folder.to_owned(),
            name: name.to_owned(),
            is_folder: true,
            size: 0,
            uploaded_by: by.to_owned(),
            uploaded_by_name: by_name.to_owned(),
            uploaded_at: ticks::unix_millis_now(),
            content_channel: 0,
        })
    }

    /// Der Eintrag zu einem importierten Inhalt; erst jetzt ist die Datei zu sehen.
    pub(crate) fn add(&mut self, record: FileRecord) -> Result<(), StorageError> {
        self.save(record)
    }

    /// Wirft einen Inhalt weg, dessen Eintrag nicht zustande kam oder der umzog.
    pub(crate) fn drop_content(&self, channel: u64, id: &str) {
        if let Ok(Some(archive)) = self.archives.existing_channel(channel) {
            let _ = archive.delete_file_content(id);
        }
    }

    /// Verschiebt und benennt zugleich um (echo-130), auch in einen anderen
    /// Kanal (echo-173); ein Ordner nimmt seinen Inhalt mit. Die Inhalte
    /// bleiben vorerst, wo sie sind; welche umziehen muessen, sagt
    /// [`Self::relocating`].
    pub(crate) fn move_to(
        &mut self,
        record: &FileRecord,
        channel: u64,
        folder: &str,
        name: &str,
    ) -> Result<(), StorageError> {
        let renamed = FileRecord {
            channel,
            folder: folder.to_owned(),
            name: name.to_owned(),
            ..record.clone()
        };
        if record.is_folder {
            let old = record.full_path();
            let new = renamed.full_path();
            for child in self.children_of(record) {
                let folder = format!("{new}{}", &child.folder[old.len()..]);
                self.save(FileRecord {
                    channel,
                    folder,
                    ..child
                })?;
            }
        }
        self.save(renamed)
    }

    /// Die Dateien, deren Inhalt noch im Archiv eines anderen Kanals liegt.
    pub(crate) fn relocating(&self) -> Vec<FileRecord> {
        self.records
            .values()
            .filter(|r| !r.is_folder && r.content_channel != r.channel)
            .cloned()
            .collect()
    }

    /// Nach [`Self::copy_content`]: der Eintrag zeigt auf die neue Kopie, die
    /// alte weicht. Wurde er inzwischen weiter verschoben oder geloescht,
    /// weicht die Kopie.
    pub(crate) fn finish_relocation(&mut self, copied: &FileRecord) -> Result<(), StorageError> {
        let current = self
            .find(&copied.id)
            .filter(|c| c.channel == copied.channel)
            .cloned();
        let Some(current) = current else {
            self.drop_content(copied.channel, &copied.id);
            return Ok(());
        };
        self.save(FileRecord {
            content_channel: copied.channel,
            ..current
        })?;
        self.drop_content(copied.content_channel, &copied.id);
        Ok(())
    }

    /// Loescht den Eintrag samt Inhalt; Ordner mit allem, was darin liegt.
    pub(crate) fn delete(&mut self, record: &FileRecord) -> Result<(), StorageError> {
        if record.is_folder {
            for child in self.children_of(record) {
                self.delete(&child)?;
            }
        }
        self.records.remove(&record.id);
        if let Some(archive) = self.archives.existing_channel(record.channel)? {
            archive.remove_file(&record.id)?;
        }
        if !record.is_folder {
            self.drop_content(record.content_channel, &record.id);
        }
        Ok(())
    }

    /// Alles im Dateibereich eines geloeschten Kanals. Eintraege und Inhalte im
    /// eigenen Archiv gehen mit dem Archiv; was noch in einem anderen liegt,
    /// wird dort weggeraeumt.
    pub(crate) fn delete_channel(&mut self, channel: u64) {
        let gone: Vec<_> = self
            .records
            .values()
            .filter(|r| r.channel == channel)
            .cloned()
            .collect();
        for record in gone {
            self.records.remove(&record.id);
            if !record.is_folder && record.content_channel != channel {
                self.drop_content(record.content_channel, &record.id);
            }
        }
    }

    /// Wie tief der tiefste Ordner unter `record` laege, wenn `record` nach
    /// `folder` kaeme; gezaehlt in Pfadteilen wie [`is_valid_folder`].
    pub(crate) fn depth_after_move(&self, record: &FileRecord, folder: &str) -> usize {
        let below = if record.is_folder {
            let own = parts(&record.full_path());
            self.children_of(record)
                .iter()
                .filter(|r| r.is_folder)
                .map(|r| parts(&r.full_path()) - own)
                .max()
                .unwrap_or(0)
        } else {
            0
        };
        parts(folder) + usize::from(record.is_folder) + below
    }

    fn children_of(&self, folder: &FileRecord) -> Vec<FileRecord> {
        let path = folder.full_path();
        let prefix = format!("{path}/");
        self.records
            .values()
            .filter(|r| {
                r.channel == folder.channel && (r.folder == path || r.folder.starts_with(&prefix))
            })
            .cloned()
            .collect()
    }

    /// Schreibt den Eintrag in das Archiv seines Kanals; wechselt er den
    /// Kanal, verlaesst er das alte.
    fn save(&mut self, record: FileRecord) -> Result<(), StorageError> {
        if let Some(old) = self
            .records
            .get(&record.id)
            .filter(|old| old.channel != record.channel)
            && let Some(archive) = self.archives.existing_channel(old.channel)?
        {
            archive.remove_file(&record.id)?;
        }
        self.archives
            .channel(record.channel)?
            .save_file(&ArchivedFile {
                id: record.id.clone(),
                folder: record.folder.clone(),
                name: record.name.clone(),
                is_folder: record.is_folder,
                size: record.size,
                uploaded_by: record.uploaded_by.clone(),
                uploaded_by_name: record.uploaded_by_name.clone(),
                uploaded_at: record.uploaded_at,
                content_channel: record.content_channel,
            })?;
        self.records.insert(record.id.clone(), record);
        Ok(())
    }
}

/// Legt den Inhalt eines fertigen Uploads ins Archiv des Kanals. Ausserhalb
/// der Sperre: bei grossen Dateien dauert das. Die Temp-Datei ist danach weg.
pub(crate) fn import(
    archives: &ArchiveSet,
    id: &str,
    channel: u64,
    temp: &Path,
) -> Result<u64, StorageError> {
    let size = std::fs::metadata(temp)?.len();
    archives.channel(channel)?.move_file_content(
        id,
        temp,
        size,
        |_| {},
        &std::sync::atomic::AtomicBool::new(false),
    )?;
    Ok(size)
}

/// Kopiert den Inhalt in das Archiv des Kanals, in dem der Eintrag jetzt
/// steht. Ausserhalb der Sperre; `false`, wenn es nichts zu kopieren gab.
pub(crate) fn copy_content(
    archives: &ArchiveSet,
    record: &FileRecord,
) -> Result<bool, StorageError> {
    let Some(source) = archives.existing_channel(record.content_channel)? else {
        return Ok(false);
    };
    let Some(mut content) = source.open_file(&record.id)? else {
        return Ok(false);
    };
    let Some(target) = archives.existing_channel(record.channel)? else {
        return Ok(false);
    };
    let length = content.len();
    target.put_file_content(&record.id, &mut content, length)?;
    Ok(true)
}

fn from_archived(file: ArchivedFile, channel: u64) -> FileRecord {
    FileRecord {
        id: file.id,
        channel,
        folder: file.folder,
        name: file.name,
        is_folder: file.is_folder,
        size: file.size,
        uploaded_by: file.uploaded_by,
        uploaded_by_name: file.uploaded_by_name,
        uploaded_at: file.uploaded_at,
        content_channel: file.content_channel,
    }
}

/// 32 Hexziffern, wie die Guids des C#-Vorgaengers.
pub(crate) fn new_id() -> String {
    let mut bytes = [0u8; ID_BYTES];
    // Ohne Zufall des Systems laeuft nichts; dann eben ein Zeitstempel.
    if getrandom::getrandom(&mut bytes).is_err() {
        bytes[..8].copy_from_slice(&ticks::now().to_le_bytes());
    }
    bytes
        .iter()
        .fold(String::with_capacity(2 * ID_BYTES), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

/// Namen ohne Pfadtrenner, Steuerzeichen und die Sonderformen "." und ".."
/// — sonst koennte ein Eintrag aus dem Kanal herauszeigen.
pub(crate) fn is_valid_name(name: &str) -> bool {
    (1..=MAX_NAME_LENGTH).contains(&name.chars().count())
        && name.trim() == name
        && !matches!(name, "." | "..")
        && !name.chars().any(|c| {
            matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
        })
}

/// Leer (Wurzel) oder gueltige Namen, getrennt durch `/`, hoechstens
/// [`MAX_DEPTH`] tief.
pub(crate) fn is_valid_folder(folder: &str) -> bool {
    folder.is_empty() || {
        let parts: Vec<_> = folder.split('/').collect();
        parts.len() <= MAX_DEPTH && parts.iter().all(|p| is_valid_name(p))
    }
}

fn parts(path: &str) -> usize {
    if path.is_empty() {
        0
    } else {
        path.split('/').count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::ServerPaths;

    fn store() -> (tempfile::TempDir, FileStore) {
        let dir = tempfile::TempDir::new().unwrap();
        let paths = ServerPaths::new(Some(dir.path().to_owned())).unwrap();
        let server = Arc::new(echo_storage::ChannelArchive::open(&paths.server_archive()).unwrap());
        let archives = Arc::new(ArchiveSet::new(paths.clone(), server).unwrap());
        std::fs::create_dir_all(paths.incoming()).unwrap();
        std::fs::write(paths.incoming().join("alt.part"), b"rest").unwrap();
        let store = FileStore::load(archives, paths.incoming(), [1, 2]).unwrap();
        assert!(!paths.incoming().join("alt.part").exists());
        (dir, store)
    }

    fn file(
        store: &mut FileStore,
        channel: u64,
        folder: &str,
        name: &str,
        content: &[u8],
    ) -> FileRecord {
        let id = new_id();
        let temp = store.new_temp_path();
        std::fs::write(&temp, content).unwrap();
        let size = import(&store.archives, &id, channel, &temp).unwrap();
        let record = FileRecord {
            id,
            channel,
            folder: folder.into(),
            name: name.into(),
            is_folder: false,
            size,
            uploaded_by: "SHA256:a".into(),
            uploaded_by_name: "Alice".into(),
            uploaded_at: 0,
            content_channel: channel,
        };
        store.add(record.clone()).unwrap();
        record
    }

    #[test]
    fn names_and_folders_stay_inside_the_channel() {
        for bad in ["", ".", "..", " x", "a/b", "a:b", "a\u{7}"] {
            assert!(!is_valid_name(bad), "{bad:?}");
        }
        assert!(is_valid_name("Bericht 2026.pdf"));
        assert!(is_valid_folder(""));
        assert!(is_valid_folder("a/b/c"));
        assert!(!is_valid_folder("a//b"));
        assert!(!is_valid_folder(&["x"; 9].join("/")));
        assert_eq!(new_id().len(), 32);
    }

    #[test]
    fn folders_move_with_their_content_and_count_depth() {
        let (_dir, mut store) = store();
        store.create_folder(1, "", "Docs", "a", "Alice").unwrap();
        store.create_folder(1, "Docs", "Alt", "a", "Alice").unwrap();
        let inner = file(&mut store, 1, "Docs/Alt", "x.txt", b"inhalt");
        assert!(store.folder_exists(1, "Docs/Alt"));
        assert!(!store.folder_exists(1, "Fehlt"));
        assert!(store.exists(1, "Docs", "ALT"));

        let docs = store.list(1, "")[0].clone();
        assert!(docs.is_folder);
        assert_eq!(store.depth_after_move(&docs, "a/b"), 4);
        store.move_to(&docs, 1, "", "Papiere").unwrap();
        assert_eq!(store.find(&inner.id).unwrap().folder, "Papiere/Alt");
        assert_eq!(store.used_bytes(), 6);
        assert!(store.has_room_for(10, 16));
        assert!(!store.has_room_for(11, 16));

        let mut content = String::new();
        std::io::Read::read_to_string(
            &mut store.open_content(&inner).unwrap().unwrap(),
            &mut content,
        )
        .unwrap();
        assert_eq!(content, "inhalt");

        let papers = store.find(&docs.id).unwrap().clone();
        store.delete(&papers).unwrap();
        assert!(store.find(&inner.id).is_none());
        assert!(!store.has_content(&inner));
    }

    #[test]
    fn contents_follow_a_move_to_another_channel() {
        let (_dir, mut store) = store();
        let record = file(&mut store, 1, "", "film.mkv", b"bilder");
        store.move_to(&record, 2, "", "film.mkv").unwrap();
        let moved = store.relocating();
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].content_channel, 1);
        assert!(store.has_content(&moved[0]));

        assert!(copy_content(&store.archives, &moved[0]).unwrap());
        store.finish_relocation(&moved[0]).unwrap();
        assert!(store.relocating().is_empty());
        let now = store.find(&record.id).unwrap().clone();
        assert_eq!(now.content_channel, 2);
        assert!(store.has_content(&now));
        assert!(store.list(1, "").is_empty());

        store.delete_channel(2);
        assert!(store.find(&record.id).is_none());
    }
}
