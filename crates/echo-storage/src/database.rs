//! Eine SQLite-Datei, geoeffnet mit turso (SQLite neu in Rust). Kapselt,
//! was echo-226 an turso gefunden hat: Pfade gehen unter Windows immer als
//! `\\?\`-Pfad hinein, sonst scheitern die langen Namen der DM-Archive.
//!
//! Nur fuer einen Prozess: turso legt keine `-shm`-Datei an, ein zweiter
//! Prozess darf ein offenes Archiv nicht anfassen. Innerhalb des Prozesses
//! lesen beliebig viele Verbindungen neben einer schreibenden (WAL).

use std::fmt;
use std::num::NonZero;
use std::path::Path;
use std::sync::Arc;

use turso_core::{Numeric, PlatformIO, SqliteDialect};

use crate::StorageError;

pub use turso_core::Value;

/// Eine offene SQLite-Datei mit ihrer ersten Verbindung.
pub struct Database {
    database: Arc<turso_core::Database>,
    connection: Connection,
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
        let connection = Connection(database.connect()?);
        Ok(Self {
            database,
            connection,
        })
    }

    /// Die erste Verbindung; sie bleibt offen, solange die Datei offen ist.
    #[must_use]
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Eine weitere Verbindung, etwa zum Lesen neben einem Schreibvorgang.
    pub fn connect(&self) -> Result<Connection, StorageError> {
        Ok(Connection(self.database.connect()?))
    }

    /// Kurz fuer `connection().query(...)`.
    pub fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<Row>, StorageError> {
        self.connection.query(sql, params)
    }

    /// Kurz fuer `connection().execute(...)`.
    pub fn execute(&self, sql: &str, params: &[Value]) -> Result<(), StorageError> {
        self.connection.execute(sql, params)
    }
}

/// Eine Verbindung zu einer offenen Datei.
#[derive(Clone)]
pub struct Connection(Arc<turso_core::Connection>);

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection").finish_non_exhaustive()
    }
}

impl Connection {
    /// Fuehrt `sql` mit den Parametern `?1`, `?2`, … aus und liefert alle Zeilen.
    pub fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<Row>, StorageError> {
        let mut statement = self.0.prepare(sql)?;
        for (index, value) in (0..)
            .map(|i| NonZero::<usize>::MIN.saturating_add(i))
            .zip(params)
        {
            statement.bind_at(index, value.clone())?;
        }
        Ok(statement.run_collect_rows()?.into_iter().map(Row).collect())
    }

    /// Wie [`Self::query`], fuer Anweisungen ohne Ergebnis.
    pub fn execute(&self, sql: &str, params: &[Value]) -> Result<(), StorageError> {
        self.query(sql, params).map(drop)
    }

    /// Die erste Zeile, wenn es eine gibt.
    pub fn query_row(&self, sql: &str, params: &[Value]) -> Result<Option<Row>, StorageError> {
        Ok(self.query(sql, params)?.into_iter().next())
    }

    /// Fuehrt `work` in einer Transaktion aus; ein Fehler darin verwirft alles.
    pub fn transaction<T>(
        &self,
        work: impl FnOnce(&Self) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        self.execute("BEGIN IMMEDIATE", &[])?;
        match work(self) {
            Ok(result) => {
                self.execute("COMMIT", &[])?;
                Ok(result)
            }
            Err(error) => {
                // Der eigentliche Fehler zaehlt; scheitert auch das Zurueckrollen,
                // ist die Verbindung ohnehin hin.
                let _ = self.execute("ROLLBACK", &[]);
                Err(error)
            }
        }
    }

    /// Inkrementeller Zugriff auf einen Blob, ohne ihn ganz zu laden.
    pub(crate) fn blob(
        &self,
        table: &str,
        column: &str,
        rowid: i64,
        write: bool,
    ) -> Result<turso_core::Blob, StorageError> {
        Ok(self.0.blob_open(table, column, rowid, write)?)
    }
}

/// Eine Ergebniszeile, Spalten in der Reihenfolge der Abfrage.
#[derive(Debug, Clone)]
pub struct Row(Vec<Value>);

impl Row {
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn value(&self, index: usize) -> Option<&Value> {
        self.0.get(index)
    }

    pub fn int(&self, index: usize) -> Result<i64, StorageError> {
        self.opt_int(index)?.ok_or(StorageError::Column {
            index,
            expected: "INTEGER",
        })
    }

    pub fn opt_int(&self, index: usize) -> Result<Option<i64>, StorageError> {
        match self.0.get(index) {
            Some(Value::Numeric(Numeric::Integer(value))) => Ok(Some(*value)),
            Some(Value::Null) => Ok(None),
            _ => Err(StorageError::Column {
                index,
                expected: "INTEGER",
            }),
        }
    }

    pub fn text(&self, index: usize) -> Result<&str, StorageError> {
        self.opt_text(index)?.ok_or(StorageError::Column {
            index,
            expected: "TEXT",
        })
    }

    pub fn opt_text(&self, index: usize) -> Result<Option<&str>, StorageError> {
        match self.0.get(index) {
            Some(Value::Null) => Ok(None),
            Some(value) => value.to_text().map(Some).ok_or(StorageError::Column {
                index,
                expected: "TEXT",
            }),
            None => Err(StorageError::Column {
                index,
                expected: "TEXT",
            }),
        }
    }

    pub fn blob(&self, index: usize) -> Result<&[u8], StorageError> {
        self.0
            .get(index)
            .and_then(Value::to_blob)
            .ok_or(StorageError::Column {
                index,
                expected: "BLOB",
            })
    }
}

impl std::ops::Index<usize> for Row {
    type Output = Value;

    fn index(&self, index: usize) -> &Value {
        &self.0[index]
    }
}

/// Parameter fuer Abfragen.
pub mod param {
    use super::Value;

    #[must_use]
    pub fn int(value: i64) -> Value {
        Value::from_i64(value)
    }

    #[must_use]
    pub fn text(value: &str) -> Value {
        Value::build_text(value.to_owned())
    }

    #[must_use]
    pub fn opt_text(value: Option<&str>) -> Value {
        value.map_or(Value::Null, text)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> (tempfile::TempDir, Database) {
        let dir = tempfile::TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("t.sqlite")).unwrap();
        (dir, db)
    }

    #[test]
    fn transactions_commit_or_roll_back() {
        let (_dir, db) = memory();
        db.execute("CREATE TABLE t(v INTEGER)", &[]).unwrap();
        let c = db.connection();

        c.transaction(|tx| tx.execute("INSERT INTO t VALUES(1)", &[]))
            .unwrap();
        let failed: Result<(), _> = c.transaction(|tx| {
            tx.execute("INSERT INTO t VALUES(2)", &[])?;
            tx.execute("KEIN SQL", &[])
        });
        assert!(failed.is_err());

        let rows = db.query("SELECT v FROM t", &[]).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].int(0).unwrap(), 1);
    }

    #[test]
    fn rows_check_their_column_types() {
        let (_dir, db) = memory();
        let row = db
            .connection()
            .query_row("SELECT 7, 'text', NULL, X'0102'", &[])
            .unwrap()
            .unwrap();
        assert_eq!(row.len(), 4);
        assert!(!row.is_empty());
        assert_eq!(row.int(0).unwrap(), 7);
        assert_eq!(row.text(1).unwrap(), "text");
        assert_eq!(row.opt_text(2).unwrap(), None);
        assert_eq!(row.opt_int(2).unwrap(), None);
        assert_eq!(row.blob(3).unwrap(), [1, 2]);
        assert!(row.int(1).is_err());
        assert!(row.int(2).is_err());
        assert!(row.text(2).is_err());
        assert!(row.text(9).is_err());
        assert!(row.blob(0).is_err());
        assert!(row.value(9).is_none());
    }

    #[test]
    fn parameters_bind_in_order() {
        let (_dir, db) = memory();
        let row = db
            .connection()
            .query_row(
                "SELECT ?1, ?2, ?3",
                &[param::int(5), param::text("a"), param::opt_text(None)],
            )
            .unwrap()
            .unwrap();
        assert_eq!(row.int(0).unwrap(), 5);
        assert_eq!(row.text(1).unwrap(), "a");
        assert_eq!(row.opt_text(2).unwrap(), None);
        assert!(db.connect().is_ok());
        assert!(format!("{db:?}{:?}", db.connection()).contains("Connection"));
    }

    #[cfg(windows)]
    #[test]
    fn local_paths_become_verbatim() {
        assert_eq!(verbatim(r"C:\daten\a.sqlar"), r"\\?\C:\daten\a.sqlar");
        assert_eq!(verbatim(r"\\nas\echo\a.sqlar"), r"\\?\UNC\nas\echo\a.sqlar");
        assert_eq!(verbatim(r"\\?\C:\a.sqlar"), r"\\?\C:\a.sqlar");
        let path = engine_path(Path::new("a.sqlar")).unwrap();
        assert!(path.starts_with(r"\\?\") && path.ends_with(r"\a.sqlar"));
    }
}
