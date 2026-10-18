//! Das sqlar-Archiv: Inhalte ablegen, in Stuecken und am Stueck, lesen und
//! darin springen, Dateien uebernehmen und dabei abbrechen.

// Hilfsfunktionen ausserhalb von #[test] zaehlen fuer clippy nicht als Test.
#![allow(clippy::unwrap_used)]
// Die Testgroessen liegen bei einigen Megabyte und passen in usize und i64.
#![allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]

use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use echo_storage::{SqlarArchive, StorageError, param};
use tempfile::TempDir;

const MTIME: i64 = 1_760_000_000;

fn archive() -> (TempDir, SqlarArchive) {
    let dir = TempDir::new().unwrap();
    let archive = SqlarArchive::open(&dir.path().join("ordner").join("a.sqlar")).unwrap();
    (dir, archive)
}

/// Ein Muster, an dem jede Stelle erkennbar ist.
fn pattern(length: u64) -> Vec<u8> {
    (0..length)
        .map(|i| ((i * 7 + i / 4096) % 251) as u8)
        .collect()
}

#[test]
fn small_contents_round_trip_and_replace() {
    let (_dir, archive) = archive();
    archive.put_bytes("images/a", b"erstes", MTIME).unwrap();
    archive.put_bytes("images/a", b"zweites", MTIME).unwrap();

    assert_eq!(archive.read_all("images/a").unwrap().unwrap(), b"zweites");
    assert_eq!(archive.length("images/a").unwrap(), Some(7));
    assert_eq!(archive.read_all("images/fehlt").unwrap(), None);
    assert_eq!(archive.length("images/fehlt").unwrap(), None);

    let row = archive
        .read(|c| {
            c.query_row(
                "SELECT mode, mtime, sz FROM sqlar WHERE name = 'images/a'",
                &[],
            )
        })
        .unwrap()
        .unwrap();
    assert_eq!(row.int(0).unwrap(), 0o100_644);
    assert_eq!(row.int(1).unwrap(), MTIME);
    assert_eq!(row.int(2).unwrap(), 7);
}

#[test]
fn large_contents_go_in_chunks_and_seek_across_them() {
    let (_dir, archive) = archive();
    let length = SqlarArchive::CHUNK_SIZE + 12_345;
    let content = pattern(length);
    archive
        .put("files/gross", &mut content.as_slice(), length, MTIME)
        .unwrap();

    let parts = archive
        .read(|c| {
            c.query(
                "SELECT name, sz FROM sqlar WHERE name LIKE '.echo/large/files/gross/%' ORDER BY name",
                &[],
            )
        })
        .unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(
        parts[1].text(0).unwrap(),
        ".echo/large/files/gross/0000000001"
    );
    assert_eq!(parts[1].int(1).unwrap(), 12_345);

    let mut reader = archive.open_read("files/gross").unwrap().unwrap();
    assert_eq!(reader.len(), length);
    assert!(!reader.is_empty());

    // Ueber die Grenze zwischen den beiden Stuecken lesen.
    let boundary = SqlarArchive::CHUNK_SIZE - 3;
    reader.seek(SeekFrom::Start(boundary)).unwrap();
    let mut across = [0u8; 8];
    reader.read_exact(&mut across).unwrap();
    assert_eq!(&across, &content[boundary as usize..boundary as usize + 8]);

    reader.seek(SeekFrom::End(-5)).unwrap();
    let mut tail = Vec::new();
    reader.read_to_end(&mut tail).unwrap();
    assert_eq!(tail, &content[content.len() - 5..]);

    reader.seek(SeekFrom::Current(-10)).unwrap();
    assert!(
        reader
            .seek(SeekFrom::Current(-(length as i64) - 1))
            .is_err()
    );

    assert_eq!(archive.read_all("files/gross").unwrap().unwrap(), content);
}

#[test]
fn a_short_source_leaves_nothing_behind() {
    let (_dir, archive) = archive();
    let length = SqlarArchive::CHUNK_SIZE + 10;
    let short = pattern(SqlarArchive::CHUNK_SIZE + 2);
    let result = archive.put("files/halb", &mut short.as_slice(), length, MTIME);
    assert!(matches!(result, Err(StorageError::ShortContent { .. })));
    assert_eq!(archive.length("files/halb").unwrap(), None);

    let small = archive.put("files/klein", &mut [1u8, 2].as_slice(), 5, MTIME);
    assert!(matches!(small, Err(StorageError::ShortContent { .. })));
    assert_eq!(archive.length("files/klein").unwrap(), None);
}

#[test]
fn delete_removes_contents_and_their_chunks() {
    let (_dir, archive) = archive();
    let length = SqlarArchive::CHUNK_SIZE + 1;
    archive
        .put("files/x", &mut pattern(length).as_slice(), length, MTIME)
        .unwrap();
    archive.put_bytes("files/y", b"y", MTIME).unwrap();

    assert!(archive.delete("files/x").unwrap());
    assert!(!archive.delete("files/x").unwrap());
    let left = archive
        .read(|c| c.query("SELECT name FROM sqlar", &[]))
        .unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].text(0).unwrap(), "files/y");
}

#[test]
fn move_in_takes_over_a_file_and_resumes_after_cancel() {
    let (dir, archive) = archive();
    let source = dir.path().join("upload.part");
    let length = 2 * SqlarArchive::CHUNK_SIZE + 99;
    let content = pattern(length);
    std::fs::write(&source, &content).unwrap();

    // Abbruch vor dem ersten Stueck: nichts passiert, die Quelle bleibt.
    let cancel = AtomicBool::new(true);
    let canceled = archive.move_in("files/up", &source, length, MTIME, |_| {}, &cancel);
    assert!(matches!(canceled, Err(StorageError::Canceled)));
    assert_eq!(std::fs::metadata(&source).unwrap().len(), length);

    // Ein Stueck uebernehmen, dann abbrechen.
    let cancel = AtomicBool::new(false);
    let mut seen = Vec::new();
    let partial = archive.move_in(
        "files/up",
        &source,
        length,
        MTIME,
        |done| {
            seen.push(done);
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        },
        &cancel,
    );
    assert!(matches!(partial, Err(StorageError::Canceled)));
    assert_eq!(seen, [99]);
    assert_eq!(
        std::fs::metadata(&source).unwrap().len(),
        2 * SqlarArchive::CHUNK_SIZE
    );

    // Weitermachen bis zum Ende; die Quelle ist danach weg.
    let resume = AtomicBool::new(false);
    let mut seen = Vec::new();
    archive
        .move_in(
            "files/up",
            &source,
            length,
            MTIME,
            |done| seen.push(done),
            &resume,
        )
        .unwrap();
    assert_eq!(seen, [SqlarArchive::CHUNK_SIZE + 99, length]);
    assert!(!source.exists());
    assert_eq!(archive.read_all("files/up").unwrap().unwrap(), content);
}

#[test]
fn move_in_of_small_files_is_a_put() {
    let (dir, archive) = archive();
    let source = dir.path().join("klein.part");
    std::fs::write(&source, b"klein").unwrap();
    let mut done = 0;
    archive
        .move_in(
            "files/k",
            &source,
            5,
            MTIME,
            |d| done = d,
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(done, 5);
    assert!(!source.exists());
    assert_eq!(archive.read_all("files/k").unwrap().unwrap(), b"klein");
}

#[test]
fn move_in_rejects_a_source_that_does_not_fit() {
    let (dir, archive) = archive();
    let source = dir.path().join("falsch.part");
    std::fs::write(&source, pattern(SqlarArchive::CHUNK_SIZE + 7)).unwrap();
    let result = archive.move_in(
        "files/f",
        &source,
        SqlarArchive::CHUNK_SIZE * 3,
        MTIME,
        |_| {},
        &AtomicBool::new(false),
    );
    assert!(matches!(result, Err(StorageError::MoveMismatch(_))));
}

#[test]
fn invalid_names_are_refused() {
    let (_dir, archive) = archive();
    assert!(matches!(
        archive.put_bytes("../raus", b"x", MTIME),
        Err(StorageError::InvalidName(_))
    ));
    assert!(archive.length("/abs").is_err());
    assert!(archive.open_read("a//b").is_err());
    assert!(archive.delete("").is_err());
}

#[test]
fn readers_see_committed_state_while_a_write_runs() {
    let (_dir, archive) = archive();
    let archive = Arc::new(archive);
    archive
        .write(|c| c.execute("CREATE TABLE chat(text TEXT)", &[]))
        .unwrap();
    archive
        .write(|c| c.execute("INSERT INTO chat VALUES(?1)", &[param::text("alt")]))
        .unwrap();

    let reader = Arc::clone(&archive);
    archive
        .write(|c| {
            c.execute("INSERT INTO chat VALUES(?1)", &[param::text("neu")])?;
            let seen = std::thread::spawn(move || {
                reader
                    .read(|r| r.query("SELECT text FROM chat", &[]))
                    .unwrap()
                    .len()
            })
            .join()
            .unwrap();
            assert_eq!(seen, 1);
            Ok(())
        })
        .unwrap();

    let after = archive
        .read(|c| c.query("SELECT text FROM chat", &[]))
        .unwrap();
    assert_eq!(after.len(), 2);
}

#[test]
fn reopened_archives_keep_their_content() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("b.sqlar");
    {
        let archive = SqlarArchive::open(&path).unwrap();
        archive.put_bytes("x", b"bleibt", MTIME).unwrap();
        assert_eq!(archive.path(), path);
        assert!(format!("{archive:?}").contains("b.sqlar"));
    }
    let archive = SqlarArchive::open(&path).unwrap();
    assert_eq!(archive.read_all("x").unwrap().unwrap(), b"bleibt");
    let page = archive
        .read(|c| c.query_row("PRAGMA page_size", &[]))
        .unwrap()
        .unwrap();
    assert_eq!(page.int(0).unwrap(), 65_536);
}
