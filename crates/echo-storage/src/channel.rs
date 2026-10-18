//! Alles Bleibende eines Kanals in einer Datei (echo-182): der Chatverlauf,
//! seine Bilder, der Dateibereich und die Tafeln (echo-183). Dient dem Server
//! als Quelle und dem Client als Spiegel. Das Server-Archiv und die Archive
//! der privaten Gespraeche haben dasselbe Format, nur ohne Dateien.
//!
//! Nachrichten werden nie geloescht; ohne Luecken ist "alles nach `seq`" die
//! ganze Synchronisation. Tabellen und Spalten sind die des C#-Vorgaengers.

use std::io::Read;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::database::{Connection, Row, param};
use crate::{ContentReader, SqlarArchive, StorageError};

/// 'echo' als `application_id`: `file` und `sqlite3` erkennen die Datei.
const APPLICATION_ID: i64 = 0x6563_686F;

/// Stand des eigenen Schemas; hoeher heisst: eine neuere echo-Version hat
/// die Datei geschrieben, lieber nicht anfassen.
const SCHEMA_VERSION: i64 = 2;

const IMAGE_PREFIX: &str = "images/";
const FILE_PREFIX: &str = "files/";
const MAX_ID_LENGTH: usize = 64;

const SCHEMA: [&str; 5] = [
    "CREATE TABLE IF NOT EXISTS messages(
        seq INTEGER PRIMARY KEY,
        time INTEGER NOT NULL,
        author TEXT NOT NULL,
        author_name TEXT NOT NULL,
        text TEXT NOT NULL,
        image TEXT)",
    "CREATE TABLE IF NOT EXISTS images(
        id TEXT PRIMARY KEY,
        width INTEGER NOT NULL,
        height INTEGER NOT NULL,
        size INTEGER NOT NULL)",
    "CREATE TABLE IF NOT EXISTS files(
        id TEXT PRIMARY KEY,
        folder TEXT NOT NULL,
        name TEXT NOT NULL,
        is_folder INTEGER NOT NULL,
        size INTEGER NOT NULL,
        uploaded_by TEXT NOT NULL,
        uploaded_by_name TEXT NOT NULL,
        uploaded_at INTEGER NOT NULL,
        content_channel INTEGER NOT NULL)",
    "CREATE TABLE IF NOT EXISTS boards(
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        created_by TEXT NOT NULL,
        created_at INTEGER NOT NULL)",
    "CREATE TABLE IF NOT EXISTS board_shapes(
        board TEXT NOT NULL,
        id TEXT NOT NULL,
        json TEXT NOT NULL,
        PRIMARY KEY(board, id))",
];

const MESSAGE_COLUMNS: &str = "seq, time, author, author_name, text, image";
const FILE_COLUMNS: &str = "id, folder, name, is_folder, size, uploaded_by, uploaded_by_name, uploaded_at, content_channel";

/// Eine gespeicherte Nachricht; `time` in Unix-Millisekunden, `author` der
/// Fingerabdruck. Mit `image` ist `text` leer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedMessage {
    pub seq: i64,
    pub time: i64,
    pub author: String,
    pub author_name: String,
    pub text: String,
    pub image: Option<String>,
}

/// Ein Bild aus dem Chat; der Inhalt liegt als sqlar-Eintrag `images/<id>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedImage {
    pub id: String,
    pub width: i64,
    pub height: i64,
    pub size: u64,
}

/// Ein Eintrag im Dateibereich; der Inhalt liegt als `files/<id>` im Archiv
/// des Kanals `content_channel` — nach dem Verschieben in einen anderen
/// Kanal bleibt er, wo er war (echo-173).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedFile {
    pub id: String,
    pub folder: String,
    pub name: String,
    pub is_folder: bool,
    pub size: u64,
    pub uploaded_by: String,
    pub uploaded_by_name: String,
    /// Unix-Millisekunden.
    pub uploaded_at: i64,
    pub content_channel: u64,
}

/// Eine Tafel im Kanal; ihre Formen stehen je als JSON daneben.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedBoard {
    pub id: String,
    pub name: String,
    pub created_by: String,
    /// Unix-Millisekunden.
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedShape {
    pub id: String,
    pub json: String,
}

#[derive(Debug)]
pub struct ChannelArchive {
    archive: SqlarArchive,
}

impl ChannelArchive {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        let archive = SqlarArchive::open(path)?;
        archive.write(|c| {
            let version = scalar(c, "PRAGMA user_version")?;
            if version > SCHEMA_VERSION {
                return Err(StorageError::NewerSchema {
                    path: path.to_owned(),
                    version,
                });
            }
            for statement in SCHEMA {
                c.execute(statement, &[])?;
            }
            c.execute(&format!("PRAGMA application_id={APPLICATION_ID}"), &[])?;
            c.execute(&format!("PRAGMA user_version={SCHEMA_VERSION}"), &[])
        })?;
        Ok(Self { archive })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        self.archive.path()
    }

    /// Die Datei darunter, fuer eigene Tabellen daneben (Konfiguration des
    /// Servers im Server-Archiv).
    #[must_use]
    pub fn sqlar(&self) -> &SqlarArchive {
        &self.archive
    }

    // --- Nachrichten ----------------------------------------------------------

    /// Server: haengt eine Nachricht an und vergibt ihre Nummer.
    pub fn append(
        &self,
        time: i64,
        author: &str,
        author_name: &str,
        text: &str,
        image: Option<&str>,
    ) -> Result<ArchivedMessage, StorageError> {
        let seq = self.archive.write(|c| {
            let seq = scalar(c, "SELECT coalesce(max(seq), 0) + 1 FROM messages")?;
            c.execute(
                &format!("INSERT INTO messages({MESSAGE_COLUMNS}) VALUES(?1, ?2, ?3, ?4, ?5, ?6)"),
                &[
                    param::int(seq),
                    param::int(time),
                    param::text(author),
                    param::text(author_name),
                    param::text(text),
                    param::opt_text(image),
                ],
            )?;
            Ok(seq)
        })?;
        Ok(ArchivedMessage {
            seq,
            time,
            author: author.to_owned(),
            author_name: author_name.to_owned(),
            text: text.to_owned(),
            image: image.map(str::to_owned),
        })
    }

    /// Client: uebernimmt Nachrichten des Servers mit dessen Nummern. Schon
    /// bekannte bleiben, wie sie sind (eine Seite kann sich ueberschneiden).
    pub fn store(&self, messages: &[ArchivedMessage]) -> Result<(), StorageError> {
        self.archive.write(|c| {
            for m in messages {
                c.execute(
                    &format!(
                        "INSERT OR IGNORE INTO messages({MESSAGE_COLUMNS}) VALUES(?1, ?2, ?3, ?4, ?5, ?6)"
                    ),
                    &[
                        param::int(m.seq),
                        param::int(m.time),
                        param::text(&m.author),
                        param::text(&m.author_name),
                        param::text(&m.text),
                        param::opt_text(m.image.as_deref()),
                    ],
                )?;
            }
            Ok(())
        })
    }

    /// Die hoechste Nummer im Archiv; 0, wenn es noch leer ist.
    pub fn last_seq(&self) -> Result<i64, StorageError> {
        self.archive
            .read(|c| scalar(c, "SELECT coalesce(max(seq), 0) FROM messages"))
    }

    /// Bis zu `limit` Nachrichten nach `seq`, aufsteigend: das Nachholen.
    pub fn after(&self, seq: i64, limit: u32) -> Result<Vec<ArchivedMessage>, StorageError> {
        self.messages(
            &format!("SELECT {MESSAGE_COLUMNS} FROM messages WHERE seq > ?1 ORDER BY seq LIMIT ?2"),
            seq,
            limit,
        )
    }

    /// Die `limit` Nachrichten direkt vor `seq`, aufsteigend: das Hochscrollen.
    pub fn before(&self, seq: i64, limit: u32) -> Result<Vec<ArchivedMessage>, StorageError> {
        let mut page = self.messages(
            &format!(
                "SELECT {MESSAGE_COLUMNS} FROM messages WHERE seq < ?1 ORDER BY seq DESC LIMIT ?2"
            ),
            seq,
            limit,
        )?;
        page.reverse();
        Ok(page)
    }

    fn messages(
        &self,
        sql: &str,
        seq: i64,
        limit: u32,
    ) -> Result<Vec<ArchivedMessage>, StorageError> {
        let rows = self
            .archive
            .read(|c| c.query(sql, &[param::int(seq), param::int(i64::from(limit))]))?;
        rows.iter().map(message).collect()
    }

    // --- Bilder ---------------------------------------------------------------

    /// Legt ein Bild ab; sichtbar wird es erst, wenn der Inhalt ganz da ist.
    pub fn put_image(
        &self,
        image: &ArchivedImage,
        content: &mut impl Read,
    ) -> Result<(), StorageError> {
        check_id(&image.id)?;
        self.archive.put(
            &format!("{IMAGE_PREFIX}{}", image.id),
            content,
            image.size,
            now_seconds(),
        )?;
        self.archive.write(|c| save_image(c, image))
    }

    /// Nur die Masse eines Bildes, ohne Inhalt: der Spiegel im Client haelt
    /// die Bilder in seinem Cache, braucht aber den Platz im Verlauf.
    pub fn remember_image(&self, image: &ArchivedImage) -> Result<(), StorageError> {
        check_id(&image.id)?;
        self.archive.write(|c| save_image(c, image))
    }

    pub fn image(&self, id: &str) -> Result<Option<ArchivedImage>, StorageError> {
        check_id(id)?;
        let row = self.archive.read(|c| {
            c.query_row(
                "SELECT width, height, size FROM images WHERE id = ?1",
                &[param::text(id)],
            )
        })?;
        row.map(|row| {
            Ok(ArchivedImage {
                id: id.to_owned(),
                width: row.int(0)?,
                height: row.int(1)?,
                size: unsigned(&row, 2)?,
            })
        })
        .transpose()
    }

    pub fn open_image(&self, id: &str) -> Result<Option<ContentReader>, StorageError> {
        if self.image(id)?.is_none() {
            return Ok(None);
        }
        self.archive.open_read(&format!("{IMAGE_PREFIX}{id}"))
    }

    // --- Dateibereich ---------------------------------------------------------

    /// Legt den Inhalt einer Datei ab, bevor ihr Eintrag gespeichert wird.
    pub fn put_file_content(
        &self,
        id: &str,
        content: &mut impl Read,
        length: u64,
    ) -> Result<(), StorageError> {
        check_id(id)?;
        self.archive.put(
            &format!("{FILE_PREFIX}{id}"),
            content,
            length,
            now_seconds(),
        )
    }

    /// Uebernimmt den Inhalt einer Datei und gibt ihren Platz dabei frei
    /// (siehe [`SqlarArchive::move_in`]); die Quelle ist danach weg.
    pub fn move_file_content(
        &self,
        id: &str,
        source: &Path,
        total: u64,
        progress: impl FnMut(u64),
        cancel: &AtomicBool,
    ) -> Result<(), StorageError> {
        check_id(id)?;
        self.archive.move_in(
            &format!("{FILE_PREFIX}{id}"),
            source,
            total,
            now_seconds(),
            progress,
            cancel,
        )
    }

    pub fn open_file(&self, id: &str) -> Result<Option<ContentReader>, StorageError> {
        check_id(id)?;
        self.archive.open_read(&format!("{FILE_PREFIX}{id}"))
    }

    /// Groesse des Inhalts einer Datei, oder `None`, wenn er hier nicht liegt.
    pub fn file_length(&self, id: &str) -> Result<Option<u64>, StorageError> {
        check_id(id)?;
        self.archive.length(&format!("{FILE_PREFIX}{id}"))
    }

    pub fn delete_file_content(&self, id: &str) -> Result<bool, StorageError> {
        check_id(id)?;
        self.archive.delete(&format!("{FILE_PREFIX}{id}"))
    }

    /// Speichert einen Eintrag neu oder ersetzt ihn (Umbenennen, Verschieben).
    pub fn save_file(&self, file: &ArchivedFile) -> Result<(), StorageError> {
        check_id(&file.id)?;
        let size = signed(file.size)?;
        let channel = signed(file.content_channel)?;
        self.archive.write(|c| {
            c.execute(
                &format!(
                    "INSERT OR REPLACE INTO files({FILE_COLUMNS}) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
                ),
                &[
                    param::text(&file.id),
                    param::text(&file.folder),
                    param::text(&file.name),
                    param::int(i64::from(file.is_folder)),
                    param::int(size),
                    param::text(&file.uploaded_by),
                    param::text(&file.uploaded_by_name),
                    param::int(file.uploaded_at),
                    param::int(channel),
                ],
            )
        })
    }

    /// Nimmt einen Eintrag heraus (geloescht oder in einen anderen Kanal
    /// gezogen); den Inhalt raeumt [`Self::delete_file_content`] weg.
    pub fn remove_file(&self, id: &str) -> Result<(), StorageError> {
        check_id(id)?;
        self.archive
            .write(|c| c.execute("DELETE FROM files WHERE id = ?1", &[param::text(id)]))
    }

    /// Alle Eintraege; filtern und sortieren tut der Aufrufer.
    pub fn files(&self) -> Result<Vec<ArchivedFile>, StorageError> {
        let rows = self
            .archive
            .read(|c| c.query(&format!("SELECT {FILE_COLUMNS} FROM files"), &[]))?;
        rows.iter()
            .map(|row| {
                Ok(ArchivedFile {
                    id: row.text(0)?.to_owned(),
                    folder: row.text(1)?.to_owned(),
                    name: row.text(2)?.to_owned(),
                    is_folder: row.int(3)? != 0,
                    size: unsigned(row, 4)?,
                    uploaded_by: row.text(5)?.to_owned(),
                    uploaded_by_name: row.text(6)?.to_owned(),
                    uploaded_at: row.int(7)?,
                    content_channel: unsigned(row, 8)?,
                })
            })
            .collect()
    }

    // --- Tafeln (echo-183) ----------------------------------------------------

    pub fn boards(&self) -> Result<Vec<ArchivedBoard>, StorageError> {
        let rows = self.archive.read(|c| {
            c.query(
                "SELECT id, name, created_by, created_at FROM boards ORDER BY created_at, id",
                &[],
            )
        })?;
        rows.iter()
            .map(|row| {
                Ok(ArchivedBoard {
                    id: row.text(0)?.to_owned(),
                    name: row.text(1)?.to_owned(),
                    created_by: row.text(2)?.to_owned(),
                    created_at: row.int(3)?,
                })
            })
            .collect()
    }

    /// Legt eine Tafel an oder benennt sie um.
    pub fn save_board(&self, board: &ArchivedBoard) -> Result<(), StorageError> {
        check_id(&board.id)?;
        self.archive.write(|c| {
            c.execute(
                "INSERT OR REPLACE INTO boards(id, name, created_by, created_at) VALUES(?1, ?2, ?3, ?4)",
                &[
                    param::text(&board.id),
                    param::text(&board.name),
                    param::text(&board.created_by),
                    param::int(board.created_at),
                ],
            )
        })
    }

    /// Die Tafel samt allen Formen.
    pub fn delete_board(&self, id: &str) -> Result<(), StorageError> {
        check_id(id)?;
        self.archive.write(|c| {
            c.execute(
                "DELETE FROM board_shapes WHERE board = ?1",
                &[param::text(id)],
            )?;
            c.execute("DELETE FROM boards WHERE id = ?1", &[param::text(id)])
        })
    }

    pub fn board_shapes(&self, board: &str) -> Result<Vec<ArchivedShape>, StorageError> {
        check_id(board)?;
        let rows = self.archive.read(|c| {
            c.query(
                "SELECT id, json FROM board_shapes WHERE board = ?1",
                &[param::text(board)],
            )
        })?;
        rows.iter()
            .map(|row| {
                Ok(ArchivedShape {
                    id: row.text(0)?.to_owned(),
                    json: row.text(1)?.to_owned(),
                })
            })
            .collect()
    }

    /// Uebernimmt Aenderungen an Formen in einer Transaktion; `None` loescht
    /// die Form.
    pub fn change_board(
        &self,
        board: &str,
        changes: &[(String, Option<String>)],
    ) -> Result<(), StorageError> {
        check_id(board)?;
        for (id, _) in changes {
            check_id(id)?;
        }
        self.archive.write(|c| {
            for (id, json) in changes {
                match json {
                    Some(json) => c.execute(
                        "INSERT OR REPLACE INTO board_shapes(board, id, json) VALUES(?1, ?2, ?3)",
                        &[param::text(board), param::text(id), param::text(json)],
                    )?,
                    None => c.execute(
                        "DELETE FROM board_shapes WHERE board = ?1 AND id = ?2",
                        &[param::text(board), param::text(id)],
                    )?,
                }
            }
            Ok(())
        })
    }
}

/// Kennungen vergibt das Programm (Guid, Hex); nur Buchstaben, Ziffern, '-'
/// und '_', damit sie als Teil eines Archivnamens sicher sind.
#[must_use]
pub fn is_valid_id(id: &str) -> bool {
    (1..=MAX_ID_LENGTH).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn check_id(id: &str) -> Result<(), StorageError> {
    if is_valid_id(id) {
        Ok(())
    } else {
        Err(StorageError::InvalidName(id.to_owned()))
    }
}

fn save_image(c: &Connection, image: &ArchivedImage) -> Result<(), StorageError> {
    c.execute(
        "INSERT OR REPLACE INTO images(id, width, height, size) VALUES(?1, ?2, ?3, ?4)",
        &[
            param::text(&image.id),
            param::int(image.width),
            param::int(image.height),
            param::int(signed(image.size)?),
        ],
    )
}

fn message(row: &Row) -> Result<ArchivedMessage, StorageError> {
    Ok(ArchivedMessage {
        seq: row.int(0)?,
        time: row.int(1)?,
        author: row.text(2)?.to_owned(),
        author_name: row.text(3)?.to_owned(),
        text: row.text(4)?.to_owned(),
        image: row.opt_text(5)?.map(str::to_owned),
    })
}

/// Die Abfragen hier liefern immer genau einen Wert.
fn scalar(c: &Connection, sql: &str) -> Result<i64, StorageError> {
    c.query_row(sql, &[])?
        .ok_or(StorageError::Column {
            index: 0,
            expected: "INTEGER",
        })?
        .int(0)
}

fn unsigned(row: &Row, index: usize) -> Result<u64, StorageError> {
    u64::try_from(row.int(index)?).map_err(|_| StorageError::Column {
        index,
        expected: "positive INTEGER",
    })
}

/// SQLite kennt nur vorzeichenbehaftete Zahlen; Groessen und Kanal-IDs passen.
fn signed(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::Column {
        index: 0,
        expected: "INTEGER bis 2^63",
    })
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_safe_archive_names() {
        assert!(is_valid_id("a1-B_c"));
        assert!(!is_valid_id(""));
        assert!(!is_valid_id("a/b"));
        assert!(!is_valid_id("ä"));
        assert!(!is_valid_id(&"x".repeat(65)));
        assert!(is_valid_id(&"x".repeat(64)));
    }
}
