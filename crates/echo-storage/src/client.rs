//! Was der Client ueber Neustarts behaelt, in einer SQLite-Datei
//! (`%AppData%\echo\client.sqlite`): Einstellungen, Favoriten und die
//! Fingerabdruecke der bekannten Server. Kein JSONL mehr; die Dateien des
//! C#-Clients werden nicht gelesen.
//!
//! Die Einstellungen selbst kennt diese Schicht nicht: sie liegen als ein
//! JSON-Dokument, das der Client schreibt und liest. Neue Felder brauchen so
//! kein neues Schema.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::StorageError;
use crate::database::{Database, Row, param};

/// 'ecli' als `application_id`, damit man die Datei von den Archiven unterscheidet.
const APPLICATION_ID: i64 = 0x6563_6C69;
const SCHEMA_VERSION: i64 = 1;

const SCHEMA: [&str; 3] = [
    "CREATE TABLE IF NOT EXISTS settings(id INTEGER PRIMARY KEY CHECK(id = 1), json TEXT NOT NULL)",
    "CREATE TABLE IF NOT EXISTS favorites(
        address TEXT PRIMARY KEY,
        position INTEGER NOT NULL,
        name TEXT NOT NULL,
        nickname TEXT NOT NULL,
        server_password TEXT,
        default_channel TEXT,
        default_channel_password TEXT)",
    "CREATE TABLE IF NOT EXISTS known_hosts(
        host TEXT PRIMARY KEY,
        fingerprint TEXT NOT NULL,
        first_seen INTEGER NOT NULL)",
];

/// Ein gespeicherter Server. Das Passwort steht im Klartext in der Datei des
/// Nutzers, wie in der bookmarks.db des Originalclients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Favorite {
    pub name: String,
    pub address: String,
    pub nickname: String,
    pub server_password: Option<String>,
    pub default_channel: Option<String>,
    pub default_channel_password: Option<String>,
}

/// Was ein Fingerabdruck fuer einen Server bedeutet (Trust on first use,
/// wie SSH, echo-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostVerdict {
    /// Derselbe wie beim ersten Mal.
    Known,
    /// Noch nie gesehen: der Nutzer entscheidet.
    New,
    /// Ein anderer als beim ersten Mal: verweigern, bis der Nutzer ihn
    /// bewusst annimmt.
    Changed,
}

#[derive(Debug)]
pub struct ClientDatabase {
    database: Database,
}

impl ClientDatabase {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let database = Database::open(path)?;
        database.connection().transaction(|c| {
            let version = c
                .query_row("PRAGMA user_version", &[])?
                .map_or(Ok(0), |row| row.int(0))?;
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
        Ok(Self { database })
    }

    // --- Einstellungen ----------------------------------------------------------

    /// Das zuletzt gespeicherte JSON-Dokument, oder `None` beim ersten Start.
    pub fn settings_json(&self) -> Result<Option<String>, StorageError> {
        self.database
            .connection()
            .query_row("SELECT json FROM settings WHERE id = 1", &[])?
            .map(|row| row.text(0).map(str::to_owned))
            .transpose()
    }

    pub fn save_settings_json(&self, json: &str) -> Result<(), StorageError> {
        self.database.execute(
            "INSERT OR REPLACE INTO settings(id, json) VALUES(1, ?1)",
            &[param::text(json)],
        )
    }

    // --- Favoriten --------------------------------------------------------------

    /// In der Reihenfolge, in der sie angelegt wurden.
    pub fn favorites(&self) -> Result<Vec<Favorite>, StorageError> {
        let rows = self.database.query(
            "SELECT name, address, nickname, server_password, default_channel, default_channel_password
             FROM favorites ORDER BY position",
            &[],
        )?;
        rows.iter().map(favorite).collect()
    }

    /// Fuegt hinzu oder ersetzt den Favoriten mit derselben Adresse; ein
    /// ersetzter behaelt seinen Platz. Adressen vergleicht sie ohne Gross-
    /// und Kleinschreibung.
    pub fn save_favorite(&self, favorite: &Favorite) -> Result<(), StorageError> {
        let key = address_key(&favorite.address);
        self.database.connection().transaction(|c| {
            let position = match c.query_row(
                "SELECT position FROM favorites WHERE address = ?1",
                &[param::text(&key)],
            )? {
                Some(row) => row.int(0)?,
                None => c
                    .query_row("SELECT coalesce(max(position), 0) + 1 FROM favorites", &[])?
                    .map_or(Ok(1), |row| row.int(0))?,
            };
            c.execute(
                "INSERT OR REPLACE INTO favorites
                 (address, position, name, nickname, server_password, default_channel, default_channel_password)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                &[
                    param::text(&key),
                    param::int(position),
                    param::text(&favorite.name),
                    param::text(&favorite.nickname),
                    param::opt_text(favorite.server_password.as_deref()),
                    param::opt_text(favorite.default_channel.as_deref()),
                    param::opt_text(favorite.default_channel_password.as_deref()),
                ],
            )
        })
    }

    pub fn remove_favorite(&self, address: &str) -> Result<(), StorageError> {
        self.database.execute(
            "DELETE FROM favorites WHERE address = ?1",
            &[param::text(&address_key(address))],
        )
    }

    // --- Bekannte Server --------------------------------------------------------

    pub fn check_host(&self, host: &str, fingerprint: &str) -> Result<HostVerdict, StorageError> {
        Ok(match self.host_fingerprint(host)? {
            None => HostVerdict::New,
            Some(known) if known == fingerprint => HostVerdict::Known,
            Some(_) => HostVerdict::Changed,
        })
    }

    /// Der gemerkte Fingerabdruck, oder `None` fuer einen unbekannten Server.
    pub fn host_fingerprint(&self, host: &str) -> Result<Option<String>, StorageError> {
        self.database
            .connection()
            .query_row(
                "SELECT fingerprint FROM known_hosts WHERE host = ?1",
                &[param::text(&address_key(host))],
            )?
            .map(|row| row.text(0).map(str::to_owned))
            .transpose()
    }

    /// Merkt sich einen Server; einen schon bekannten ersetzt das, weil der
    /// Nutzer den neuen Schluessel bewusst angenommen hat.
    pub fn remember_host(&self, host: &str, fingerprint: &str) -> Result<(), StorageError> {
        self.database.execute(
            "INSERT OR REPLACE INTO known_hosts(host, fingerprint, first_seen) VALUES(?1, ?2, ?3)",
            &[
                param::text(&address_key(host)),
                param::text(fingerprint),
                param::int(now_millis()),
            ],
        )
    }

    pub fn forget_host(&self, host: &str) -> Result<(), StorageError> {
        self.database.execute(
            "DELETE FROM known_hosts WHERE host = ?1",
            &[param::text(&address_key(host))],
        )
    }
}

/// Adressen und Hosts gelten ohne Gross- und Kleinschreibung als gleich, wie
/// DNS-Namen; gespeichert wird darum die kleine Form.
fn address_key(address: &str) -> String {
    address.trim().to_lowercase()
}

fn favorite(row: &Row) -> Result<Favorite, StorageError> {
    let optional = |index| row.opt_text(index).map(|t| t.map(str::to_owned));
    Ok(Favorite {
        name: row.text(0)?.to_owned(),
        address: row.text(1)?.to_owned(),
        nickname: row.text(2)?.to_owned(),
        server_password: optional(3)?,
        default_channel: optional(4)?,
        default_channel_password: optional(5)?,
    })
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
