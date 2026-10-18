//! Der lokale Spiegel der Verlaeufe eines Servers (echo-182): dieselben
//! Archive wie dort, mit den Nummern des Servers, aber nur Nachrichten und
//! die Masse der Bilder. Mit dem Spiegel ist der Verlauf gleich beim Start
//! da, und nachgeholt wird nur, was nach der hoechsten Nummer kommt.
//!
//! Der Spiegel ist nur ein Cache: faellt er aus, laeuft der Chat ohne ihn.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use echo_storage::{
    ArchivedImage, ArchivedMessage, ChannelArchive, StorageError, file_key, fingerprint,
};
use parking_lot::Mutex;

use crate::chat::{ChatArchive, ChatScope};

const EXTENSION: &str = "sqlar";

/// Eine Nachricht mit ihrem Bild, wie der Server sie liefert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) message: ArchivedMessage,
    pub(crate) image: Option<ArchivedImage>,
}

#[derive(Debug)]
pub(crate) struct ChatMirror {
    dir: PathBuf,
    open: Mutex<HashMap<ChatArchive, Arc<ChannelArchive>>>,
}

impl ChatMirror {
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            open: Mutex::new(HashMap::new()),
        }
    }

    /// Uebernimmt Nachrichten des Servers; schon bekannte bleiben.
    pub(crate) fn store(
        &self,
        archive: &ChatArchive,
        entries: &[Entry],
    ) -> Result<(), StorageError> {
        if entries.is_empty() {
            return Ok(());
        }
        let Some(store) = self.open(archive, true)? else {
            return Ok(());
        };
        for image in entries.iter().filter_map(|e| e.image.as_ref()) {
            store.remember_image(image)?;
        }
        let messages: Vec<_> = entries.iter().map(|e| e.message.clone()).collect();
        store.store(&messages)
    }

    /// Die hoechste Nummer, die hier liegt; 0 fuer ein unbekanntes Archiv.
    pub(crate) fn last_seq(&self, archive: &ChatArchive) -> i64 {
        self.open(archive, false)
            .ok()
            .flatten()
            .and_then(|store| store.last_seq().ok())
            .unwrap_or(0)
    }

    /// Die letzten `count` Nachrichten vor `before`.
    pub(crate) fn before(&self, archive: &ChatArchive, before: i64, count: u32) -> Vec<Entry> {
        let Ok(Some(store)) = self.open(archive, false) else {
            return Vec::new();
        };
        store
            .before(before, count)
            .unwrap_or_default()
            .into_iter()
            .map(|message| {
                let image = message
                    .image
                    .as_deref()
                    .and_then(|id| store.image(id).ok().flatten());
                Entry { message, image }
            })
            .collect()
    }

    /// Die privaten Gespraeche, die hier liegen.
    pub(crate) fn conversations(&self) -> Vec<ChatArchive> {
        let Ok(entries) = std::fs::read_dir(self.dir.join("dm")) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .filter_map(|e| archive_stem(&e.path()))
            .filter_map(|stem| fingerprint(&stem))
            .map(|peer| ChatArchive::private(&peer))
            .collect()
    }

    fn open(
        &self,
        archive: &ChatArchive,
        create: bool,
    ) -> Result<Option<Arc<ChannelArchive>>, StorageError> {
        let Some(path) = self.path_of(archive) else {
            return Ok(None);
        };
        let mut open = self.open.lock();
        if let Some(store) = open.get(archive) {
            return Ok(Some(Arc::clone(store)));
        }
        if !create && !path.exists() {
            return Ok(None);
        }
        let store = Arc::new(ChannelArchive::open(&path)?);
        open.insert(archive.clone(), Arc::clone(&store));
        Ok(Some(store))
    }

    fn path_of(&self, archive: &ChatArchive) -> Option<PathBuf> {
        match archive.scope {
            ChatScope::Server => Some(self.dir.join(format!("server.{EXTENSION}"))),
            ChatScope::Channel => Some(
                self.dir
                    .join("channels")
                    .join(format!("{}.{EXTENSION}", archive.channel)),
            ),
            ChatScope::Private => file_key(&archive.peer)
                .map(|key| self.dir.join("dm").join(format!("{key}.{EXTENSION}"))),
            ChatScope::System => None,
        }
    }
}

fn archive_stem(path: &Path) -> Option<String> {
    (path.extension()? == EXTENSION)
        .then(|| path.file_stem()?.to_str().map(str::to_owned))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOB: &str = "SHA256:ypeBEsobvcr6wjGzmiPcTaeG7/gUfE5yuYB3ha/uSLs";

    fn entry(seq: i64, image: bool) -> Entry {
        Entry {
            message: ArchivedMessage {
                seq,
                time: 1_760_000_000_000,
                author: BOB.into(),
                author_name: "Bob".into(),
                text: format!("n{seq}"),
                image: image.then(|| "img".to_owned()),
            },
            image: image.then(|| ArchivedImage {
                id: "img".into(),
                width: 1,
                height: 2,
                size: 3,
            }),
        }
    }

    #[test]
    fn keeps_server_numbers_and_images() {
        let dir = tempfile::TempDir::new().unwrap();
        let mirror = ChatMirror::new(dir.path().to_owned());
        let dm = ChatArchive::private(BOB);
        assert_eq!(mirror.last_seq(&dm), 0);
        assert!(mirror.before(&dm, i64::MAX, 10).is_empty());

        mirror
            .store(&dm, &[entry(4, false), entry(5, true)])
            .unwrap();
        mirror.store(&dm, &[]).unwrap();
        assert_eq!(mirror.last_seq(&dm), 5);
        let page = mirror.before(&dm, i64::MAX, 10);
        assert_eq!(page.len(), 2);
        assert_eq!(page[1].image.as_ref().unwrap().height, 2);
        assert_eq!(mirror.conversations(), [dm]);

        let channel = ChatArchive::channel(3);
        mirror.store(&channel, &[entry(1, false)]).unwrap();
        assert_eq!(mirror.last_seq(&channel), 1);
        mirror
            .store(&ChatArchive::server(), &[entry(1, false)])
            .unwrap();
        assert_eq!(mirror.last_seq(&ChatArchive::server()), 1);
        assert_eq!(mirror.last_seq(&ChatArchive::private("kein")), 0);
    }
}
