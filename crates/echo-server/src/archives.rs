//! Die Archive des Servers (echo-182): eines fuer den Serverchat (es haelt
//! auch die Konfiguration, siehe [`crate::stores::ServerDatabase`]), eines je
//! Kanal und eines je privatem Gespraech. Geoeffnet wird bei Bedarf, dann
//! bleiben sie offen. Namen und Ordner wie beim C#-Vorgaenger.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use echo_storage::{ChannelArchive, StorageError, file_key, fingerprint};
use parking_lot::Mutex;

use crate::paths::{ServerPaths, archive_extension};

const DM_SEPARATOR: char = '.';

#[derive(Debug)]
pub(crate) struct ArchiveSet {
    paths: ServerPaths,
    server: Arc<ChannelArchive>,
    channels: Mutex<HashMap<u64, Arc<ChannelArchive>>>,
    dms: Mutex<HashMap<String, Arc<ChannelArchive>>>,
}

impl ArchiveSet {
    pub(crate) fn new(paths: ServerPaths, server: Arc<ChannelArchive>) -> std::io::Result<Self> {
        std::fs::create_dir_all(paths.channel_archives())?;
        std::fs::create_dir_all(paths.dm_archives())?;
        Ok(Self {
            paths,
            server,
            channels: Mutex::new(HashMap::new()),
            dms: Mutex::new(HashMap::new()),
        })
    }

    /// Der Serverchat.
    #[must_use]
    pub(crate) fn server(&self) -> &Arc<ChannelArchive> {
        &self.server
    }

    /// Das Archiv eines Kanals; legt es an, wenn es noch keines gibt.
    pub(crate) fn channel(&self, id: u64) -> Result<Arc<ChannelArchive>, StorageError> {
        let mut channels = self.channels.lock();
        if let Some(archive) = channels.get(&id) {
            return Ok(Arc::clone(archive));
        }
        let archive = Arc::new(ChannelArchive::open(&self.channel_path(id))?);
        channels.insert(id, Arc::clone(&archive));
        Ok(archive)
    }

    /// Wie [`Self::channel`], aber `None`, solange es fuer den Kanal keines
    /// gibt (etwa, weil er inzwischen geloescht ist).
    pub(crate) fn existing_channel(
        &self,
        id: u64,
    ) -> Result<Option<Arc<ChannelArchive>>, StorageError> {
        if self.channels.lock().contains_key(&id) || self.channel_path(id).exists() {
            return self.channel(id).map(Some);
        }
        Ok(None)
    }

    /// Das Archiv des Gespraechs zweier Fingerabdruecke; mit `create` false
    /// `None`, solange es keines gibt. Auch `None`, wenn einer der beiden
    /// kein gueltiger Fingerabdruck ist.
    pub(crate) fn dm(
        &self,
        a: &str,
        b: &str,
        create: bool,
    ) -> Result<Option<Arc<ChannelArchive>>, StorageError> {
        let Some(name) = dm_name(a, b) else {
            return Ok(None);
        };
        let mut dms = self.dms.lock();
        if let Some(archive) = dms.get(&name) {
            return Ok(Some(Arc::clone(archive)));
        }
        let path = self
            .paths
            .dm_archives()
            .join(format!("{name}.{}", archive_extension()));
        if !create && !path.exists() {
            return Ok(None);
        }
        let archive = Arc::new(ChannelArchive::open(&path)?);
        dms.insert(name, Arc::clone(&archive));
        Ok(Some(archive))
    }

    /// Mit wem `fingerprint` private Gespraeche hat, laut Ordner.
    #[must_use]
    pub(crate) fn peers_of(&self, own_fingerprint: &str) -> Vec<String> {
        let Some(own) = file_key(own_fingerprint) else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(self.paths.dm_archives()) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| archive_stem(&entry.path()))
            .filter_map(|stem| {
                let (left, right) = stem.split_once(DM_SEPARATOR)?;
                let other = if left == own {
                    right
                } else if right == own {
                    left
                } else {
                    return None;
                };
                fingerprint(other)
            })
            .collect()
    }

    /// Ein Kanal ist weg. Das Archiv eines temporaeren Kanals verschwindet mit
    /// ihm; das eines dauerhaften kommt in den Papierkorb, geloescht wird nur
    /// vom Betreiber.
    pub(crate) fn retire_channel(&self, id: u64, permanent: bool) {
        // Erst schliessen, sonst laesst sich die Datei unter Windows nicht bewegen.
        drop(self.channels.lock().remove(&id));
        let path = self.channel_path(id);
        if !path.exists() {
            return;
        }
        if permanent {
            self.trash(&path);
        } else {
            delete_with_sides(&path);
        }
    }

    /// Beim Start: Archive von Kanaelen, die es nicht mehr gibt, in den
    /// Papierkorb. Temporaere Kanaele ueberleben keinen Neustart, und ihre
    /// Nummern werden neu vergeben — ein neuer Kanal darf nicht den Verlauf
    /// eines alten erben.
    pub(crate) fn retire_orphans(&self, channels: &HashSet<u64>) {
        let Ok(entries) = std::fs::read_dir(self.paths.channel_archives()) else {
            return;
        };
        for path in entries.filter_map(Result::ok).map(|e| e.path()) {
            let Some(stem) = archive_stem(&path) else {
                continue;
            };
            if !stem.parse::<u64>().is_ok_and(|id| channels.contains(&id)) {
                self.trash(&path);
            }
        }
    }

    fn channel_path(&self, id: u64) -> PathBuf {
        self.paths
            .channel_archives()
            .join(format!("{id}.{}", archive_extension()))
    }

    fn trash(&self, path: &Path) {
        let trash = self.paths.archive_trash();
        if std::fs::create_dir_all(&trash).is_err() {
            return;
        }
        let stem = archive_stem(path).unwrap_or_default();
        let stamp = crate::ticks::display(crate::ticks::now())
            .replace([' ', ':', '-'], "")
            .trim_end_matches('Z')
            .to_owned();
        let target = trash.join(format!("{stem}-{stamp}.{}", archive_extension()));
        move_with_sides(path, &target);
    }
}

/// Der Dateiname ohne Endung, wenn es ein Archiv ist.
fn archive_stem(path: &Path) -> Option<String> {
    (path.extension()? == archive_extension())
        .then(|| path.file_stem()?.to_str().map(str::to_owned))
        .flatten()
}

fn dm_name(a: &str, b: &str) -> Option<String> {
    let (x, y) = (file_key(a)?, file_key(b)?);
    Some(if x <= y {
        format!("{x}{DM_SEPARATOR}{y}")
    } else {
        format!("{y}{DM_SEPARATOR}{x}")
    })
}

/// Die Datei samt WAL. Haelt sie noch jemand (ein laufender Download),
/// bleibt sie liegen: der naechste Start raeumt sie.
fn move_with_sides(path: &Path, target: &Path) {
    if std::fs::rename(path, target).is_ok() {
        for side in ["-wal", "-shm"] {
            let from = side_path(path, side);
            if from.exists() {
                let _ = std::fs::rename(from, side_path(target, side));
            }
        }
    }
}

fn delete_with_sides(path: &Path) {
    for file in [
        path.to_owned(),
        side_path(path, "-wal"),
        side_path(path, "-shm"),
    ] {
        let _ = std::fs::remove_file(file);
    }
}

fn side_path(path: &Path, side: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(side);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALICE: &str = "SHA256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU";
    const BOB: &str = "SHA256:ypeBEsobvcr6wjGzmiPcTaeG7/gUfE5yuYB3ha/uSLs";

    fn set() -> (tempfile::TempDir, ArchiveSet) {
        let dir = tempfile::TempDir::new().unwrap();
        let paths = ServerPaths::new(Some(dir.path().to_owned())).unwrap();
        let server = Arc::new(ChannelArchive::open(&paths.server_archive()).unwrap());
        (dir, ArchiveSet::new(paths, server).unwrap())
    }

    #[test]
    fn dm_archives_are_named_like_in_csharp_and_found_from_both_sides() {
        let (dir, set) = set();
        assert!(set.dm(ALICE, BOB, false).unwrap().is_none());
        let dm = set.dm(BOB, ALICE, true).unwrap().unwrap();
        dm.append(1, ALICE, "Alice", "hi", None).unwrap();
        let same = set.dm(ALICE, BOB, false).unwrap().unwrap();
        assert_eq!(same.last_seq().unwrap(), 1);
        assert!(set.dm(ALICE, "kein fingerabdruck", true).unwrap().is_none());

        let expected = dir.path().join("archives/dm").join(
            "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb.e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855.sqlar",
        );
        assert!(expected.exists());
        assert_eq!(set.peers_of(ALICE), [BOB]);
        assert_eq!(set.peers_of(BOB), [ALICE]);
        assert!(set.peers_of("kein").is_empty());
    }

    #[test]
    fn retired_channels_go_to_trash_or_away() {
        let (dir, set) = set();
        set.channel(5)
            .unwrap()
            .append(1, ALICE, "Alice", "bleibt", None)
            .unwrap();
        set.channel(6).unwrap();
        set.retire_channel(5, true);
        set.retire_channel(6, false);
        set.retire_channel(7, true);

        let channels = dir.path().join("archives/channels");
        assert!(!channels.join("5.sqlar").exists());
        assert!(!channels.join("6.sqlar").exists());
        let trashed: Vec<_> = std::fs::read_dir(dir.path().join("archives/trash"))
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| archive_stem(Path::new(n)).is_some())
            .collect();
        assert_eq!(trashed.len(), 1);
        assert!(trashed[0].starts_with("5-"));
    }

    #[test]
    fn orphans_of_vanished_channels_are_retired_at_start() {
        let (dir, set) = set();
        set.channel(1).unwrap();
        set.channel(9).unwrap();
        drop(set);

        let paths = ServerPaths::new(Some(dir.path().to_owned())).unwrap();
        let server = Arc::new(ChannelArchive::open(&paths.server_archive()).unwrap());
        let set = ArchiveSet::new(paths, server).unwrap();
        set.retire_orphans(&HashSet::from([1]));
        assert!(dir.path().join("archives/channels/1.sqlar").exists());
        assert!(!dir.path().join("archives/channels/9.sqlar").exists());
        assert!(set.server().last_seq().is_ok());
    }
}
