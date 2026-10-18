//! Eine SQLite-Datei, geoeffnet mit turso (SQLite neu in Rust). Kapselt,
//! was echo-226 an turso gefunden hat: Pfade gehen unter Windows immer als
//! `\\?\`-Pfad hinein, sonst scheitern die langen Namen der DM-Archive.
//!
//! Nur fuer einen Prozess: turso legt keine `-shm`-Datei an, ein zweiter
//! Prozess darf ein offenes Archiv nicht anfassen.

use std::fmt;
use std::num::NonZero;
use std::path::Path;
use std::sync::Arc;

use turso_core::{Connection, PlatformIO, SqliteDialect};

use crate::StorageError;

pub use turso_core::Value;

/// Eine Ergebniszeile, Spalten in der Reihenfolge der Abfrage.
pub type Row = Vec<Value>;

/// Eine offene SQLite-Datei mit einer Verbindung.
pub struct Database {
    connection: Arc<Connection>,
}

impl fmt::Debug for Database {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Database").finish_non_exhaustive()
    }
}

impl Database {
    /// Oeffnet `path` oder legt die Datei an.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        let io = Arc::new(PlatformIO::new()?);
        let database =
            turso_core::Database::open_file(io, &engine_path(path)?, Arc::new(SqliteDialect))?;
        Ok(Self {
            connection: database.connect()?,
        })
    }

    /// Fuehrt `sql` mit den Parametern `?1`, `?2`, … aus und liefert alle Zeilen.
    pub fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<Row>, StorageError> {
        let mut statement = self.connection.prepare(sql)?;
        for (index, value) in (0..)
            .map(|i| NonZero::<usize>::MIN.saturating_add(i))
            .zip(params)
        {
            statement.bind_at(index, value.clone())?;
        }
        Ok(statement.run_collect_rows()?)
    }

    /// Wie [`Self::query`], fuer Anweisungen ohne Ergebnis.
    pub fn execute(&self, sql: &str, params: &[Value]) -> Result<(), StorageError> {
        self.query(sql, params).map(drop)
    }
}

/// Der Pfad, wie turso ihn braucht: absolut, und unter Windows mit `\\?\`,
/// weil turso die Grenze von 260 Zeichen sonst nicht ueberschreitet.
fn engine_path(path: &Path) -> Result<String, StorageError> {
    let absolute = std::path::absolute(path)?;
    let text = absolute
        .to_str()
        .ok_or_else(|| StorageError::PathNotUnicode(absolute.clone()))?;
    Ok(verbatim(text))
}

#[cfg(windows)]
fn verbatim(path: &str) -> String {
    if path.starts_with(r"\\?\") {
        path.to_owned()
    } else if let Some(share) = path.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{share}")
    } else {
        format!(r"\\?\{path}")
    }
}

#[cfg(not(windows))]
fn verbatim(path: &str) -> String {
    path.to_owned()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn local_paths_become_verbatim() {
        assert_eq!(verbatim(r"C:\daten\a.sqlar"), r"\\?\C:\daten\a.sqlar");
    }

    #[test]
    fn shares_become_verbatim_unc() {
        assert_eq!(verbatim(r"\\nas\echo\a.sqlar"), r"\\?\UNC\nas\echo\a.sqlar");
    }

    #[test]
    fn verbatim_paths_stay() {
        assert_eq!(verbatim(r"\\?\C:\a.sqlar"), r"\\?\C:\a.sqlar");
    }

    #[test]
    fn relative_paths_become_absolute() {
        let path = engine_path(Path::new("a.sqlar")).unwrap();
        assert!(path.starts_with(r"\\?\") && path.ends_with(r"\a.sqlar"));
    }
}
