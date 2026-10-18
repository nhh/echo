//! Eine SQLite-Datei im Standardformat sqlar (<https://sqlite.org/sqlar.html>),
//! wie `sqlite3 -A` sie liest: jede Datei ist eine Zeile
//! `sqlar(name, mode, mtime, sz, data)` (echo-182). Eigene Tabellen daneben
//! legt der Aufrufer an; diese Schicht kuemmert sich um Binaerdaten und den
//! Zugriff.
//!
//! Wie beim C#-Vorgaenger: grosse Seiten, WAL, Inhalte per `zeroblob` und
//! inkrementellem Blob-Schreiben statt als Ganzes gebunden, und Dateien ueber
//! [`SqlarArchive::CHUNK_SIZE`] als Folge von Zeilen unter
//! [`SqlarArchive::LARGE_PREFIX`] — keine Zeile sprengt die BLOB-Grenze, und
//! jede bleibt fuer `sqlite3 -A` lesbar. Inhalte liegen roh (`sz =
//! length(data)`): Bilder und Filme sind schon gepackt, und nur so laesst
//! sich an jeder Stelle lesen.
//!
//! Geschrieben wird ueber genau eine Verbindung unter einer Sperre, gelesen
//! ueber eigene Verbindungen, damit ein laufender Film nicht auf den Chat
//! wartet.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::StorageError;
use crate::content::{ContentReader, Segment};
use crate::database::{Connection, Database, param};

/// rw-r--r-- einer regulaeren Datei, wie `sqlite3 -A` es setzt (0100644).
const REGULAR_FILE: i64 = 0o100_644;
const COPY_BUFFER_SIZE: usize = 1 << 16;
const INDEX_DIGITS: usize = 10;
/// Grosse Seiten fuer Zeilen voller Blobs; nur vor dem ersten Schreiben aenderbar.
const PAGE_SIZE: u32 = 65_536;

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS sqlar(name TEXT PRIMARY KEY, mode INT, mtime INT, sz INT, data BLOB);
    CREATE INDEX IF NOT EXISTS sqlar_meta ON sqlar(name, mode, mtime, sz);";

pub struct SqlarArchive {
    path: PathBuf,
    database: Database,
    /// Eine Schreibverbindung, und immer nur ein Schreiber.
    writer: Mutex<()>,
}

impl std::fmt::Debug for SqlarArchive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlarArchive")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl SqlarArchive {
    /// Groesste Zeile; groessere Inhalte werden zerlegt.
    pub const CHUNK_SIZE: u64 = 16 << 20;

    /// Namensraum der Stuecke grosser Inhalte: `.echo/large/<name>/<index>`.
    pub const LARGE_PREFIX: &'static str = ".echo/large/";

    pub fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let is_new = std::fs::metadata(path).map_or(true, |m| m.len() == 0);
        let database = Database::open(path)?;
        if is_new {
            database.execute(&format!("PRAGMA page_size={PAGE_SIZE}"), &[])?;
        }
        database.execute("PRAGMA journal_mode=WAL", &[])?;
        for statement in SCHEMA.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            database.execute(statement, &[])?;
        }
        Ok(Self {
            path: path.to_owned(),
            database,
            writer: Mutex::new(()),
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    // --- Schreiben ------------------------------------------------------------

    /// Fuehrt `work` in einer Transaktion der Schreibverbindung aus; so legt
    /// ein Aufrufer auch seine eigenen Tabellen an. Ein Fehler verwirft alles.
    pub fn write<T>(
        &self,
        work: impl FnOnce(&Connection) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        // Ein Schreiber, der in Panik geriet, hat seine Transaktion nicht
        // festgeschrieben; die Sperre selbst bleibt brauchbar.
        let _guard = self
            .writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.database.connection().transaction(work)
    }

    /// Legt `content` unter `name` ab und ersetzt einen alten Inhalt. Grosse
    /// Inhalte gehen Stueck fuer Stueck, jedes in einer eigenen Transaktion:
    /// dazwischen kommen andere Schreiber (Chat) dran, und das WAL bleibt
    /// klein. Wer den Inhalt erst nach dem Ende sehen soll, traegt ihn danach
    /// in seine eigene Tabelle ein.
    pub fn put(
        &self,
        name: &str,
        content: &mut impl Read,
        length: u64,
        modified_unix_seconds: i64,
    ) -> Result<(), StorageError> {
        check_name(name)?;
        if length <= Self::CHUNK_SIZE {
            return self.write(|c| {
                remove(c, name)?;
                put_row(c, name, modified_unix_seconds, content, length)
            });
        }

        self.write(|c| remove(c, name).map(drop))?;
        let result = (|| {
            let mut remaining = length;
            let mut index = 0;
            while remaining > 0 {
                let part = remaining.min(Self::CHUNK_SIZE);
                let part_name = part_name(name, index);
                self.write(|c| put_row(c, &part_name, modified_unix_seconds, content, part))?;
                remaining -= part;
                index += 1;
            }
            Ok(())
        })();
        if result.is_err() {
            // Die schon festgeschriebenen Stuecke waeren ein halber Inhalt.
            let _ = self.write(|c| remove(c, name));
        }
        result
    }

    pub fn put_bytes(
        &self,
        name: &str,
        content: &[u8],
        modified_unix_seconds: i64,
    ) -> Result<(), StorageError> {
        self.put(
            name,
            &mut &content[..],
            content.len() as u64,
            modified_unix_seconds,
        )
    }

    /// Uebernimmt eine Datei und gibt dabei ihren Platz frei: Stueck fuer
    /// Stueck von hinten, jedes festgeschrieben, bevor die Quelle um genau
    /// dieses Stueck gekuerzt wird. So braucht es nur ein Stueck Platz extra,
    /// auch fuer eine Datei, die groesser ist als der freie Platz (echo-182).
    /// `total` ist die urspruengliche Groesse.
    ///
    /// Abgebrochen (auch mit `cancel` zwischen zwei Stuecken) macht der
    /// naechste Aufruf dort weiter, wo die Quelle jetzt endet; am Ende ist sie
    /// weg. `progress` meldet nach jedem Stueck, wie viel schon im Archiv liegt.
    pub fn move_in(
        &self,
        name: &str,
        source_path: &Path,
        total: u64,
        modified_unix_seconds: i64,
        mut progress: impl FnMut(u64),
        cancel: &AtomicBool,
    ) -> Result<(), StorageError> {
        check_name(name)?;
        if total <= Self::CHUNK_SIZE {
            self.put(
                name,
                &mut File::open(source_path)?,
                total,
                modified_unix_seconds,
            )?;
            std::fs::remove_file(source_path)?;
            progress(total);
            return Ok(());
        }

        let parts = total.div_ceil(Self::CHUNK_SIZE);
        let mut source = OpenOptions::new()
            .read(true)
            .write(true)
            .open(source_path)?;
        let present = source.metadata()?.len();
        if present == total {
            // Frisch angefangen: ein alter Inhalt gleichen Namens weicht.
            self.write(|c| remove(c, name).map(drop))?;
        }

        for index in (0..present.div_ceil(Self::CHUNK_SIZE)).rev() {
            if cancel.load(Ordering::Relaxed) {
                return Err(StorageError::Canceled);
            }
            let start = index * Self::CHUNK_SIZE;
            let length = if index == parts - 1 {
                total - start
            } else {
                Self::CHUNK_SIZE
            };
            if start + length != source.metadata()?.len() {
                return Err(StorageError::MoveMismatch(source_path.to_owned()));
            }

            source.seek(SeekFrom::Start(start))?;
            let part_name = part_name(name, index);
            self.write(|c| put_row(c, &part_name, modified_unix_seconds, &mut source, length))?;
            source.set_len(start)?;
            source.sync_all()?;
            progress(total - start);
        }

        drop(source);
        std::fs::remove_file(source_path)?;
        Ok(())
    }

    /// Entfernt einen Inhalt samt Stuecken; `false`, wenn es ihn nicht gab.
    pub fn delete(&self, name: &str) -> Result<bool, StorageError> {
        check_name(name)?;
        self.write(|c| remove(c, name))
    }

    // --- Lesen ----------------------------------------------------------------

    /// Liest ueber eine eigene Verbindung, neben laufenden Schreibvorgaengen.
    pub fn read<T>(
        &self,
        work: impl FnOnce(&Connection) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        work(&self.database.connect()?)
    }

    /// Groesse eines Inhalts, oder `None`, wenn es ihn nicht gibt.
    pub fn length(&self, name: &str) -> Result<Option<u64>, StorageError> {
        check_name(name)?;
        let segments = self.read(|c| segments(c, name))?;
        Ok((!segments.is_empty()).then(|| segments.iter().map(|s| s.length).sum()))
    }

    /// Ein Lesestrom, der sich an jede Stelle setzen laesst, oder `None`. Er
    /// haelt seine eigene Verbindung, bis er geschlossen wird.
    pub fn open_read(&self, name: &str) -> Result<Option<ContentReader>, StorageError> {
        check_name(name)?;
        let connection = self.database.connect()?;
        let segments = segments(&connection, name)?;
        Ok((!segments.is_empty()).then(|| ContentReader::new(connection, segments)))
    }

    /// Den ganzen Inhalt auf einmal, fuer kleine Dinge wie Chatbilder.
    pub fn read_all(&self, name: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let Some(mut reader) = self.open_read(name)? else {
            return Ok(None);
        };
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Ok(Some(bytes))
    }
}

fn put_row(
    c: &Connection,
    name: &str,
    mtime: i64,
    content: &mut impl Read,
    length: u64,
) -> Result<(), StorageError> {
    let size = i64::try_from(length).map_err(|_| StorageError::InvalidName(name.to_owned()))?;
    c.execute(
        "INSERT INTO sqlar(name, mode, mtime, sz, data) VALUES(?1, ?2, ?3, ?4, zeroblob(?4))
         ON CONFLICT(name) DO UPDATE SET mode=excluded.mode, mtime=excluded.mtime, sz=excluded.sz, data=excluded.data",
        &[param::text(name), param::int(REGULAR_FILE), param::int(mtime), param::int(size)],
    )?;
    let rowid = c
        .query_row(
            "SELECT rowid FROM sqlar WHERE name = ?1",
            &[param::text(name)],
        )?
        .ok_or_else(|| StorageError::InvalidName(name.to_owned()))?
        .int(0)?;

    let mut blob = c.blob("sqlar", "data", rowid, true)?;
    let mut buffer = vec![0u8; COPY_BUFFER_SIZE];
    let mut written: u64 = 0;
    while written < length {
        let want = usize::try_from((length - written).min(COPY_BUFFER_SIZE as u64))
            .unwrap_or(COPY_BUFFER_SIZE);
        let read = content.read(&mut buffer[..want])?;
        if read == 0 {
            let _ = blob.close();
            return Err(StorageError::ShortContent {
                name: name.to_owned(),
                written,
                length,
            });
        }
        // Blobs passen in usize: eine Zeile ist hoechstens CHUNK_SIZE gross.
        let offset = usize::try_from(written).unwrap_or(usize::MAX);
        blob.write(offset, &buffer[..read])?;
        written += read as u64;
    }
    blob.close()?;
    Ok(())
}

fn remove(c: &Connection, name: &str) -> Result<bool, StorageError> {
    let (low, high) = part_bounds(name);
    let before = count(c)?;
    c.execute(
        "DELETE FROM sqlar WHERE name = ?1 OR (name >= ?2 AND name < ?3)",
        &[param::text(name), param::text(&low), param::text(&high)],
    )?;
    Ok(count(c)? < before)
}

fn count(c: &Connection) -> Result<i64, StorageError> {
    c.query_row("SELECT count(*) FROM sqlar", &[])?
        .map_or(Ok(0), |row| row.int(0))
}

/// Die Zeilen eines Inhalts in Reihenfolge: eine fuer kleine, die Stuecke
/// fuer grosse.
fn segments(c: &Connection, name: &str) -> Result<Vec<Segment>, StorageError> {
    let single = c.query(
        "SELECT rowid, sz, length(data) FROM sqlar WHERE name = ?1",
        &[param::text(name)],
    )?;
    let rows = if single.is_empty() {
        let (low, high) = part_bounds(name);
        c.query(
            "SELECT rowid, sz, length(data) FROM sqlar WHERE name >= ?1 AND name < ?2 ORDER BY name",
            &[param::text(&low), param::text(&high)],
        )?
    } else {
        single
    };

    rows.iter()
        .map(|row| {
            let size = row.int(1)?;
            if row.int(2)? != size {
                return Err(StorageError::Compressed(name.to_owned()));
            }
            Ok(Segment {
                rowid: row.int(0)?,
                length: u64::try_from(size)
                    .map_err(|_| StorageError::Compressed(name.to_owned()))?,
            })
        })
        .collect()
}

fn part_name(name: &str, index: u64) -> String {
    format!(
        "{}{name}/{index:0INDEX_DIGITS$}",
        SqlarArchive::LARGE_PREFIX
    )
}

/// Alle Stuecke eines Namens als Bereich ueber den Index: ':' folgt in
/// ASCII direkt auf '9', deckt also jeden Ziffernindex ab.
fn part_bounds(name: &str) -> (String, String) {
    let low = format!("{}{name}/", SqlarArchive::LARGE_PREFIX);
    let high = format!("{low}:");
    (low, high)
}

/// Namen kommen aus Kennungen des Programms, nie vom Nutzer; trotzdem nichts,
/// was beim Entpacken mit `sqlite3 -A` aus dem Ordner zeigt.
fn check_name(name: &str) -> Result<(), StorageError> {
    let bad = name.is_empty()
        || name.starts_with('/')
        || name.starts_with(SqlarArchive::LARGE_PREFIX)
        || name.split('/').any(|part| matches!(part, "" | "." | ".."));
    if bad {
        Err(StorageError::InvalidName(name.to_owned()))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_stay_inside_the_archive() {
        for bad in ["", "/abs", "a//b", "a/./b", "../x", ".echo/large/x"] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
        assert!(check_name("files/abc").is_ok());
    }

    #[test]
    fn part_names_sort_by_index() {
        assert_eq!(part_name("files/x", 3), ".echo/large/files/x/0000000003");
        let (low, high) = part_bounds("files/x");
        assert!(low.as_str() < part_name("files/x", 0).as_str());
        assert!(part_name("files/x", 9_999_999_999).as_str() < high.as_str());
    }
}
