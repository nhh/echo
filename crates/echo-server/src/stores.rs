//! Die Konfiguration des Servers als Tabellen im Server-Archiv
//! (`archives/server.sqlar`, echo-190), neben dem Serverchat. Tabellen,
//! Spalten und Zeitformate sind die des C#-Vorgaengers.

use std::collections::HashMap;
use std::sync::Arc;

use echo_protocol::{ApprovalState, Role, Wire};
use echo_storage::{ChannelArchive, Connection, SqlarArchive, StorageError, param};
use serde::{Deserialize, Serialize};

use crate::ticks;

/// Was der Betreiber einstellt; eine Zeile in der Tabelle `settings`, als
/// JSON in camelCase mit Enums als Text, wie beim C#-Vorgaenger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerSettings {
    pub name: String,
    pub welcome_message: String,
    pub port: u16,
    /// Serverpasswort im Klartext (die Datei gehoert dem Betreiber); `None` = keines.
    pub password: Option<String>,
    /// Unbekannte Schluessel muessen erst freigeschaltet werden; bekannte
    /// Mitglieder kommen weiter durch, damit das Einschalten niemanden
    /// aussperrt (echo-39).
    pub require_approval: bool,
    /// Rolle fuer unbekannte Schluessel.
    pub default_role: Role,
    pub max_clients: u32,
    /// Obergrenze fuer alle Dateien zusammen (echo-20).
    pub max_file_bytes: u64,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            name: "echo".into(),
            welcome_message: "Willkommen.".into(),
            port: Wire::DEFAULT_PORT,
            password: None,
            require_approval: false,
            default_role: Role::Member,
            max_clients: 128,
            max_file_bytes: 25 * 1024 * 1024 * 1024,
        }
    }
}

/// Ein dauerhafter Kanal, wie er in der Tabelle `channels` steht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredChannel {
    pub(crate) id: u64,
    pub(crate) parent: u64,
    pub(crate) order: u64,
    pub(crate) name: String,
    pub(crate) topic: Option<String>,
    pub(crate) password: Option<String>,
    pub(crate) is_default: bool,
    pub(crate) max_clients: i32,
    pub(crate) needed_talk_power: i32,
}

/// Ein bekannter Schluessel mit Rolle und letztem Namen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Member {
    pub(crate) fingerprint: String,
    pub(crate) role: Role,
    pub(crate) last_name: String,
    /// .NET-Ticks UTC.
    pub(crate) last_seen: i64,
}

/// Ein Bann; aufgehoben ist er, wenn `until` vorbei ist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BanEntry {
    pub(crate) fingerprint: String,
    pub(crate) name: String,
    pub(crate) reason: String,
    /// .NET-Ticks UTC; [`ticks::FOREVER`] fuer immer.
    pub(crate) until: i64,
    pub(crate) by: String,
}

/// Wer angeklopft hat und wie entschieden wurde (echo-39).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Approval {
    pub(crate) fingerprint: String,
    pub(crate) name: String,
    pub(crate) state: ApprovalState,
    /// .NET-Ticks UTC.
    pub(crate) requested: i64,
    pub(crate) decided: i64,
    pub(crate) decided_by: String,
}

const SCHEMA: [&str; 6] = [
    "CREATE TABLE IF NOT EXISTS settings(id INTEGER PRIMARY KEY CHECK(id = 1), json TEXT NOT NULL)",
    "CREATE TABLE IF NOT EXISTS channels(
        id INTEGER PRIMARY KEY, parent INTEGER NOT NULL, ord INTEGER NOT NULL, name TEXT NOT NULL, topic TEXT,
        password TEXT, is_default INTEGER NOT NULL, max_clients INTEGER NOT NULL, needed_talk_power INTEGER NOT NULL)",
    "CREATE TABLE IF NOT EXISTS members(fingerprint TEXT PRIMARY KEY, role INTEGER NOT NULL, last_name TEXT NOT NULL, last_seen INTEGER NOT NULL)",
    "CREATE TABLE IF NOT EXISTS bans(fingerprint TEXT PRIMARY KEY, name TEXT NOT NULL, reason TEXT NOT NULL, until INTEGER NOT NULL, by TEXT NOT NULL)",
    "CREATE TABLE IF NOT EXISTS approvals(
        fingerprint TEXT PRIMARY KEY, name TEXT NOT NULL, state INTEGER NOT NULL,
        requested INTEGER NOT NULL, decided INTEGER NOT NULL, decided_by TEXT NOT NULL)",
    "CREATE TABLE IF NOT EXISTS events(id INTEGER PRIMARY KEY, time INTEGER NOT NULL, kind TEXT NOT NULL, text TEXT NOT NULL)",
];

/// Die Konfiguration im Server-Archiv. Mitglieder, Banns und Freigaben liegen
/// zusaetzlich im Speicher, weil jeder Verbindungsaufbau sie fragt.
#[derive(Debug)]
pub(crate) struct ServerDatabase {
    archive: Arc<ChannelArchive>,
    members: HashMap<String, Member>,
    bans: HashMap<String, BanEntry>,
    approvals: HashMap<String, Approval>,
}

impl ServerDatabase {
    pub(crate) fn open(archive: Arc<ChannelArchive>) -> Result<Self, StorageError> {
        archive.sqlar().write(|c| {
            for statement in SCHEMA {
                c.execute(statement, &[])?;
            }
            Ok(())
        })?;
        let mut database = Self {
            archive,
            members: HashMap::new(),
            bans: HashMap::new(),
            approvals: HashMap::new(),
        };
        database.load()?;
        Ok(database)
    }

    fn db(&self) -> &SqlarArchive {
        self.archive.sqlar()
    }

    fn load(&mut self) -> Result<(), StorageError> {
        let members = self.db().read(|c| {
            c.query(
                "SELECT fingerprint, role, last_name, last_seen FROM members",
                &[],
            )
        })?;
        for row in members {
            let member = Member {
                fingerprint: row.text(0)?.to_owned(),
                role: role_from(row.int(1)?),
                last_name: row.text(2)?.to_owned(),
                last_seen: row.int(3)?,
            };
            self.members.insert(member.fingerprint.clone(), member);
        }

        let bans = self
            .db()
            .read(|c| c.query("SELECT fingerprint, name, reason, until, by FROM bans", &[]))?;
        for row in bans {
            let ban = BanEntry {
                fingerprint: row.text(0)?.to_owned(),
                name: row.text(1)?.to_owned(),
                reason: row.text(2)?.to_owned(),
                until: row.int(3)?,
                by: row.text(4)?.to_owned(),
            };
            self.bans.insert(ban.fingerprint.clone(), ban);
        }

        let approvals = self.db().read(|c| {
            c.query(
                "SELECT fingerprint, name, state, requested, decided, decided_by FROM approvals",
                &[],
            )
        })?;
        for row in approvals {
            let approval = Approval {
                fingerprint: row.text(0)?.to_owned(),
                name: row.text(1)?.to_owned(),
                state: approval_state_from(row.int(2)?),
                requested: row.int(3)?,
                decided: row.int(4)?,
                decided_by: row.text(5)?.to_owned(),
            };
            self.approvals
                .insert(approval.fingerprint.clone(), approval);
        }
        Ok(())
    }

    // --- Einstellungen --------------------------------------------------------

    /// Die gespeicherten Einstellungen, oder `None` auf einem frischen Server.
    pub(crate) fn settings(&self) -> Result<Option<ServerSettings>, StorageError> {
        let row = self
            .db()
            .read(|c| c.query_row("SELECT json FROM settings WHERE id = 1", &[]))?;
        row.map(|row| {
            serde_json::from_str(row.text(0)?)
                .map_err(|e| StorageError::Io(std::io::Error::other(e)))
        })
        .transpose()
    }

    pub(crate) fn save_settings(&self, settings: &ServerSettings) -> Result<(), StorageError> {
        let json = serde_json::to_string(settings)
            .map_err(|e| StorageError::Io(std::io::Error::other(e)))?;
        self.db().write(|c| {
            c.execute(
                "INSERT OR REPLACE INTO settings(id, json) VALUES(1, ?1)",
                &[param::text(&json)],
            )
        })
    }

    // --- Kanaele --------------------------------------------------------------

    pub(crate) fn channels(&self) -> Result<Vec<StoredChannel>, StorageError> {
        let rows = self.db().read(|c| {
            c.query(
                "SELECT id, parent, ord, name, topic, password, is_default, max_clients, needed_talk_power
                 FROM channels ORDER BY id",
                &[],
            )
        })?;
        rows.iter()
            .map(|row| {
                Ok(StoredChannel {
                    id: unsigned(row.int(0)?),
                    parent: unsigned(row.int(1)?),
                    order: unsigned(row.int(2)?),
                    name: row.text(3)?.to_owned(),
                    topic: row.opt_text(4)?.map(str::to_owned),
                    password: row.opt_text(5)?.map(str::to_owned),
                    is_default: row.int(6)? != 0,
                    max_clients: narrow(row.int(7)?),
                    needed_talk_power: narrow(row.int(8)?),
                })
            })
            .collect()
    }

    /// Schreibt die dauerhaften Kanaele ganz neu.
    pub(crate) fn save_channels(&self, channels: &[StoredChannel]) -> Result<(), StorageError> {
        self.db().write(|c| {
            c.execute("DELETE FROM channels", &[])?;
            for channel in channels {
                insert_channel(c, channel)?;
            }
            Ok(())
        })
    }

    // --- Mitglieder -----------------------------------------------------------

    #[must_use]
    pub(crate) fn member(&self, fingerprint: &str) -> Option<&Member> {
        self.members.get(fingerprint)
    }

    #[must_use]
    pub(crate) fn has_owner(&self) -> bool {
        self.members.values().any(|m| m.role == Role::Owner)
    }

    pub(crate) fn save_member(&mut self, member: Member) -> Result<(), StorageError> {
        self.db().write(|c| {
            c.execute(
                "INSERT OR REPLACE INTO members(fingerprint, role, last_name, last_seen) VALUES(?1, ?2, ?3, ?4)",
                &[
                    param::text(&member.fingerprint),
                    param::int(member.role as i64),
                    param::text(&member.last_name),
                    param::int(member.last_seen),
                ],
            )
        })?;
        self.members.insert(member.fingerprint.clone(), member);
        Ok(())
    }

    // --- Banns ----------------------------------------------------------------

    /// Ein Bann, der noch gilt.
    #[must_use]
    pub(crate) fn active_ban(&self, fingerprint: &str) -> Option<&BanEntry> {
        self.bans
            .get(fingerprint)
            .filter(|ban| ban.until > ticks::now())
    }

    pub(crate) fn add_ban(&mut self, ban: BanEntry) -> Result<(), StorageError> {
        self.db().write(|c| {
            c.execute(
                "INSERT OR REPLACE INTO bans(fingerprint, name, reason, until, by) VALUES(?1, ?2, ?3, ?4, ?5)",
                &[
                    param::text(&ban.fingerprint),
                    param::text(&ban.name),
                    param::text(&ban.reason),
                    param::int(ban.until),
                    param::text(&ban.by),
                ],
            )
        })?;
        self.bans.insert(ban.fingerprint.clone(), ban);
        Ok(())
    }

    // --- Freigaben ------------------------------------------------------------

    #[must_use]
    pub(crate) fn approval(&self, fingerprint: &str) -> Option<&Approval> {
        self.approvals.get(fingerprint)
    }

    pub(crate) fn approvals(&self) -> impl Iterator<Item = &Approval> {
        self.approvals.values()
    }

    #[must_use]
    pub(crate) fn pending_approvals(&self) -> u32 {
        let pending = self
            .approvals
            .values()
            .filter(|a| a.state == ApprovalState::Pending)
            .count();
        u32::try_from(pending).unwrap_or(u32::MAX)
    }

    pub(crate) fn save_approval(&mut self, approval: Approval) -> Result<(), StorageError> {
        self.db().write(|c| {
            c.execute(
                "INSERT OR REPLACE INTO approvals(fingerprint, name, state, requested, decided, decided_by)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                &[
                    param::text(&approval.fingerprint),
                    param::text(&approval.name),
                    param::int(approval.state as i64),
                    param::int(approval.requested),
                    param::int(approval.decided),
                    param::text(&approval.decided_by),
                ],
            )
        })?;
        self.approvals
            .insert(approval.fingerprint.clone(), approval);
        Ok(())
    }

    // --- Ereignisse -----------------------------------------------------------

    /// Haengt ein Ereignis an. Scheitert das (volles Laufwerk), laeuft der
    /// Server trotzdem weiter.
    pub(crate) fn log_event(&self, kind: &str, text: &str) {
        let _ = self.db().write(|c| {
            c.execute(
                "INSERT INTO events(time, kind, text) VALUES(?1, ?2, ?3)",
                &[
                    param::int(ticks::now()),
                    param::text(kind),
                    param::text(text),
                ],
            )
        });
    }
}

fn insert_channel(c: &Connection, channel: &StoredChannel) -> Result<(), StorageError> {
    c.execute(
        "INSERT INTO channels(id, parent, ord, name, topic, password, is_default, max_clients, needed_talk_power)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        &[
            param::int(signed(channel.id)),
            param::int(signed(channel.parent)),
            param::int(signed(channel.order)),
            param::text(&channel.name),
            param::opt_text(channel.topic.as_deref()),
            param::opt_text(channel.password.as_deref()),
            param::int(i64::from(channel.is_default)),
            param::int(i64::from(channel.max_clients)),
            param::int(i64::from(channel.needed_talk_power)),
        ],
    )
}

/// Rollen stehen als Zahl in der Datei, in der Reihenfolge der Rangfolge.
fn role_from(value: i64) -> Role {
    // Unbekannte Werte nie hoeher einstufen als noetig.
    match value {
        1 => Role::Member,
        2 => Role::Admin,
        3 => Role::Owner,
        _ => Role::Guest,
    }
}

fn approval_state_from(value: i64) -> ApprovalState {
    match value {
        1 => ApprovalState::Allowed,
        2 => ApprovalState::Denied,
        _ => ApprovalState::Pending,
    }
}

/// Kanal-IDs sind positiv; SQLite kennt nur vorzeichenbehaftete Zahlen.
fn unsigned(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

fn signed(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn narrow(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> (tempfile::TempDir, ServerDatabase) {
        let dir = tempfile::TempDir::new().unwrap();
        let archive = Arc::new(ChannelArchive::open(&dir.path().join("server.sqlar")).unwrap());
        (dir, ServerDatabase::open(archive).unwrap())
    }

    #[test]
    fn settings_round_trip_as_camel_case_json() {
        let (_dir, db) = database();
        assert_eq!(db.settings().unwrap(), None);
        let settings = ServerSettings {
            name: "Test".into(),
            password: Some("pw".into()),
            default_role: Role::Guest,
            ..ServerSettings::default()
        };
        db.save_settings(&settings).unwrap();
        assert_eq!(db.settings().unwrap(), Some(settings));
        let json: String = db
            .db()
            .read(|c| c.query_row("SELECT json FROM settings", &[]))
            .unwrap()
            .unwrap()
            .text(0)
            .unwrap()
            .to_owned();
        assert!(json.contains(r#""welcomeMessage""#) && json.contains(r#""defaultRole":"Guest""#));
    }

    #[test]
    fn members_bans_and_approvals_survive_a_reopen() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("server.sqlar");
        {
            let mut db =
                ServerDatabase::open(Arc::new(ChannelArchive::open(&path).unwrap())).unwrap();
            assert!(!db.has_owner());
            db.save_member(Member {
                fingerprint: "SHA256:a".into(),
                role: Role::Owner,
                last_name: "Alice".into(),
                last_seen: ticks::now(),
            })
            .unwrap();
            db.add_ban(BanEntry {
                fingerprint: "SHA256:m".into(),
                name: "Mallory".into(),
                reason: "Spam".into(),
                until: ticks::FOREVER,
                by: "SHA256:a".into(),
            })
            .unwrap();
            db.add_ban(BanEntry {
                fingerprint: "SHA256:old".into(),
                name: "Alt".into(),
                reason: "vorbei".into(),
                until: ticks::now() - 1,
                by: "SHA256:a".into(),
            })
            .unwrap();
            db.save_approval(Approval {
                fingerprint: "SHA256:e".into(),
                name: "Eve".into(),
                state: ApprovalState::Pending,
                requested: ticks::now(),
                decided: 0,
                decided_by: String::new(),
            })
            .unwrap();
            db.log_event("test", "Eintrag");
        }

        let db = ServerDatabase::open(Arc::new(ChannelArchive::open(&path).unwrap())).unwrap();
        assert!(db.has_owner());
        assert_eq!(db.member("SHA256:a").unwrap().last_name, "Alice");
        assert!(db.active_ban("SHA256:m").is_some());
        assert!(db.active_ban("SHA256:old").is_none());
        assert_eq!(db.pending_approvals(), 1);
        assert_eq!(db.approvals().count(), 1);
        assert_eq!(db.approval("SHA256:e").unwrap().name, "Eve");
    }

    #[test]
    fn channels_are_rewritten_as_a_whole() {
        let (_dir, db) = database();
        let channel = |id, name: &str| StoredChannel {
            id,
            parent: 0,
            order: 0,
            name: name.into(),
            topic: None,
            password: None,
            is_default: id == 1,
            max_clients: -1,
            needed_talk_power: 0,
        };
        db.save_channels(&[channel(1, "Lobby"), channel(2, "Zwei")])
            .unwrap();
        db.save_channels(&[channel(1, "Lobby")]).unwrap();
        assert_eq!(db.channels().unwrap(), [channel(1, "Lobby")]);
    }

    #[test]
    fn unknown_numbers_map_to_the_safe_side() {
        assert_eq!(role_from(9), Role::Guest);
        assert_eq!(role_from(3), Role::Owner);
        assert_eq!(approval_state_from(9), ApprovalState::Pending);
        assert_eq!(narrow(i64::MAX), i32::MAX);
        assert_eq!(narrow(i64::MIN), i32::MIN);
        assert_eq!(unsigned(-1), 0);
    }
}
