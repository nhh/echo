//! Liest und schreibt Archive, die der C#-Server geschrieben hat
//! (`tests/fixtures/csharp`, erzeugt mit `tools/gen-csharp-fixtures.cs`).
//! Jeder Test arbeitet auf einer Kopie unter einem absichtlich langen Pfad,
//! damit auch die Grenze von 260 Zeichen unter Windows mitgeprueft wird.

// Hilfsfunktionen ausserhalb von #[test] zaehlen fuer clippy nicht als Test.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

use echo_storage::{Database, Value, file_key};
use tempfile::TempDir;

/// Der C#-Server legt Gespraeche zwischen diesen beiden ab.
const ALICE: &str = "SHA256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU";
const BOB: &str = "SHA256:ypeBEsobvcr6wjGzmiPcTaeG7/gUfE5yuYB3ha/uSLs";

/// Zusammen mit den 130 Zeichen eines DM-Archivs ueber 260 Zeichen.
const LONG_FOLDER: &str = "ein-ordner-mit-einem-sehr-langen-namen-damit-der-pfad-zum-dm-archiv-ueber-die-grenze-von-windows-kommt";

struct Fixtures {
    _dir: TempDir,
    root: PathBuf,
}

impl Fixtures {
    fn copy() -> Self {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join(LONG_FOLDER);
        copy_tree(&fixture_root(), &root);
        Self { _dir: dir, root }
    }

    fn open(&self, relative: &str) -> Database {
        Database::open(&self.root.join(relative)).unwrap()
    }
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/csharp")
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn text(value: &Value) -> String {
    value.to_text().unwrap().to_owned()
}

fn int(value: &Value) -> i64 {
    value.to_string().parse().unwrap()
}

fn single(db: &Database, sql: &str) -> Value {
    db.query(sql, &[]).unwrap()[0][0].clone()
}

#[test]
fn reads_server_configuration() {
    let fixtures = Fixtures::copy();
    let db = fixtures.open("archives/server.sqlar");

    let settings = text(&single(&db, "SELECT json FROM settings WHERE id = 1"));
    assert!(settings.contains(r#""name":"Testserver""#));
    assert!(settings.contains(r#""defaultRole":"Guest""#));

    let channels = db
        .query("SELECT name, parent FROM channels ORDER BY id", &[])
        .unwrap();
    let names: Vec<_> = channels.iter().map(|row| text(&row[0])).collect();
    assert_eq!(names, ["Lobby", "Geschützt", "Unterkanal"]);
    assert_eq!(int(&channels[2][1]), 2);

    assert_eq!(int(&single(&db, "SELECT count(*) FROM members")), 2);
    assert_eq!(int(&single(&db, "SELECT count(*) FROM approvals")), 1);
}

#[test]
fn server_tables_keep_dotnet_ticks() {
    let fixtures = Fixtures::copy();
    let db = fixtures.open("archives/server.sqlar");

    // 2026-10-01 12:00 UTC in .NET-Ticks, so wie der C#-Server es schrieb.
    assert_eq!(
        int(&single(&db, "SELECT time FROM events ORDER BY id LIMIT 1")),
        639_264_528_000_000_000
    );
}

#[test]
fn reads_channel_chat_in_pages() {
    let fixtures = Fixtures::copy();
    let db = fixtures.open("archives/channels/1.sqlar");

    let page = db
        .query(
            "SELECT seq, text FROM messages WHERE seq < ?1 ORDER BY seq DESC LIMIT ?2",
            &[Value::from_i64(40), Value::from_i64(3)],
        )
        .unwrap();
    let seqs: Vec<_> = page.iter().map(|row| int(&row[0])).collect();
    assert_eq!(seqs, [39, 38, 37]);
    assert_eq!(text(&page[0][1]), "Nachricht 38 äöüß");
}

#[test]
fn reads_images_and_files_from_sqlar() {
    let fixtures = Fixtures::copy();
    let db = fixtures.open("archives/channels/1.sqlar");

    let png = single(&db, "SELECT data FROM sqlar WHERE name = 'images/img01'");
    assert_eq!(&png.to_blob().unwrap()[1..4], b"PNG");

    let file = single(&db, "SELECT data FROM sqlar WHERE name = 'files/file01'");
    let bytes = file.to_blob().unwrap();
    assert_eq!(bytes.len(), 100_000);
    assert!(
        bytes
            .iter()
            .enumerate()
            .all(|(i, b)| u64::from(*b) == (i as u64 * 31) % 256)
    );
}

#[test]
fn reads_whiteboards() {
    let fixtures = Fixtures::copy();
    let db = fixtures.open("archives/channels/1.sqlar");

    let shapes = db
        .query(
            "SELECT id, json FROM board_shapes WHERE board = 'board01' ORDER BY id",
            &[],
        )
        .unwrap();
    assert_eq!(shapes.len(), 2);
    assert_eq!(text(&shapes[1][1]), r#"{"kind":"text","text":"Hallo"}"#);
}

#[test]
fn opens_dm_archives_beyond_the_windows_path_limit() {
    let fixtures = Fixtures::copy();
    let name = format!(
        "{}.{}.sqlar",
        file_key(BOB).unwrap(),
        file_key(ALICE).unwrap()
    );
    let path = fixtures.root.join("archives/dm").join(name);
    assert!(path.as_os_str().len() > 260);

    let db = Database::open(&path).unwrap();
    assert_eq!(text(&single(&db, "SELECT text FROM messages")), "Privat");
}

#[test]
fn replays_an_archive_that_was_not_closed() {
    let fixtures = Fixtures::copy();
    let db = fixtures.open("crash/live.sqlar");

    assert_eq!(
        text(&single(&db, "SELECT text FROM messages")),
        "nur im WAL"
    );
}

#[test]
fn writes_what_reads_back_after_reopening() {
    let fixtures = Fixtures::copy();
    let path = fixtures.root.join("archives/channels/1.sqlar");
    {
        let db = Database::open(&path).unwrap();
        db.execute(
            "INSERT INTO messages(seq, time, author, author_name, text, image) \
             VALUES((SELECT max(seq) + 1 FROM messages), ?1, ?2, 'Rust', 'aus Rust', NULL)",
            &[Value::from_i64(1_760_000_000_000), Value::build_text(ALICE)],
        )
        .unwrap();
    }

    let db = Database::open(&path).unwrap();
    let last = db
        .query(
            "SELECT seq, text FROM messages ORDER BY seq DESC LIMIT 1",
            &[],
        )
        .unwrap();
    assert_eq!(int(&last[0][0]), 52);
    assert_eq!(text(&last[0][1]), "aus Rust");
}

#[test]
fn reports_sql_errors_as_values() {
    let fixtures = Fixtures::copy();
    let db = fixtures.open("archives/server.sqlar");

    assert!(db.query("SELECT * FROM gibt_es_nicht", &[]).is_err());
}

#[test]
fn sqlar_archive_reads_what_the_csharp_server_stored() {
    let fixtures = Fixtures::copy();
    let archive =
        echo_storage::SqlarArchive::open(&fixtures.root.join("archives/channels/1.sqlar")).unwrap();

    let file = archive.read_all("files/file01").unwrap().unwrap();
    assert_eq!(file.len(), 100_000);
    assert!(
        file.iter()
            .enumerate()
            .all(|(i, b)| u64::from(*b) == (i as u64 * 31) % 256)
    );
    assert_eq!(
        &archive.read_all("images/img01").unwrap().unwrap()[1..4],
        b"PNG"
    );

    archive
        .put_bytes("files/rust01", b"aus Rust", 1_760_000_000)
        .unwrap();
    assert_eq!(
        archive.read_all("files/rust01").unwrap().unwrap(),
        b"aus Rust"
    );
}
