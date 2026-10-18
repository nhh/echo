//! Das Archiv eines Kanals: Chat mit lueckenlosen Nummern, Bilder, Dateien
//! und Tafeln, neu angelegt und in den Archiven des C#-Servers.

// Hilfsfunktionen ausserhalb von #[test] zaehlen fuer clippy nicht als Test.
#![allow(clippy::unwrap_used)]

use std::io::Read;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use echo_storage::{
    ArchivedBoard, ArchivedFile, ArchivedImage, ArchivedMessage, ChannelArchive, StorageError,
};
use tempfile::TempDir;

const ALICE: &str = "SHA256:alice";
const T0: i64 = 1_760_000_000_000;

fn archive() -> (TempDir, ChannelArchive) {
    let dir = TempDir::new().unwrap();
    let archive = ChannelArchive::open(&dir.path().join("1.sqlar")).unwrap();
    (dir, archive)
}

fn texts(messages: &[ArchivedMessage]) -> Vec<&str> {
    messages.iter().map(|m| m.text.as_str()).collect()
}

#[test]
fn chat_numbers_have_no_gaps_and_page_both_ways() {
    let (_dir, archive) = archive();
    assert_eq!(archive.last_seq().unwrap(), 0);
    for i in 1..=5 {
        let m = archive
            .append(T0 + i, ALICE, "Alice", &format!("n{i}"), None)
            .unwrap();
        assert_eq!(m.seq, i);
    }
    assert_eq!(archive.last_seq().unwrap(), 5);
    assert_eq!(texts(&archive.after(2, 2).unwrap()), ["n3", "n4"]);
    assert_eq!(texts(&archive.before(5, 2).unwrap()), ["n3", "n4"]);
    assert_eq!(texts(&archive.before(2, 10).unwrap()), ["n1"]);
    assert!(archive.after(5, 10).unwrap().is_empty());
}

#[test]
fn mirrors_store_server_messages_without_duplicates() {
    let (_dir, mirror) = archive();
    let message = |seq: i64, text: &str| ArchivedMessage {
        seq,
        time: T0,
        author: ALICE.into(),
        author_name: "Alice".into(),
        text: text.into(),
        image: None,
    };
    mirror.store(&[message(7, "a"), message(8, "b")]).unwrap();
    mirror
        .store(&[message(8, "anders"), message(9, "c")])
        .unwrap();
    assert_eq!(texts(&mirror.after(0, 10).unwrap()), ["a", "b", "c"]);
}

#[test]
fn images_appear_only_with_their_content() {
    let (_dir, archive) = archive();
    let image = ArchivedImage {
        id: "img1".into(),
        width: 2,
        height: 3,
        size: 4,
    };
    archive
        .put_image(&image, &mut [1u8, 2, 3, 4].as_slice())
        .unwrap();
    archive
        .append(T0, ALICE, "Alice", "", Some("img1"))
        .unwrap();

    assert_eq!(archive.image("img1").unwrap(), Some(image.clone()));
    let mut bytes = Vec::new();
    archive
        .open_image("img1")
        .unwrap()
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, [1, 2, 3, 4]);
    assert_eq!(
        archive.after(0, 1).unwrap()[0].image.as_deref(),
        Some("img1")
    );

    let remembered = ArchivedImage {
        id: "img2".into(),
        ..image
    };
    archive.remember_image(&remembered).unwrap();
    assert!(archive.image("img2").unwrap().is_some());
    assert!(archive.open_image("fehlt").unwrap().is_none());
    assert!(archive.image("a/b").is_err());
}

#[test]
fn files_keep_entry_and_content_apart() {
    let (dir, archive) = archive();
    let file = ArchivedFile {
        id: "f1".into(),
        folder: "/".into(),
        name: "x.bin".into(),
        is_folder: false,
        size: 3,
        uploaded_by: ALICE.into(),
        uploaded_by_name: "Alice".into(),
        uploaded_at: T0,
        content_channel: 1,
    };
    archive
        .put_file_content("f1", &mut b"abc".as_slice(), 3)
        .unwrap();
    archive.save_file(&file).unwrap();
    archive
        .save_file(&ArchivedFile {
            name: "y.bin".into(),
            ..file.clone()
        })
        .unwrap();

    let files = archive.files().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "y.bin");
    assert_eq!(archive.file_length("f1").unwrap(), Some(3));
    let mut content = String::new();
    archive
        .open_file("f1")
        .unwrap()
        .unwrap()
        .read_to_string(&mut content)
        .unwrap();
    assert_eq!(content, "abc");

    let source = dir.path().join("f2.part");
    std::fs::write(&source, b"hochgeladen").unwrap();
    archive
        .move_file_content("f2", &source, 11, |_| {}, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(archive.file_length("f2").unwrap(), Some(11));

    archive.remove_file("f1").unwrap();
    assert!(archive.files().unwrap().is_empty());
    assert!(archive.delete_file_content("f1").unwrap());
    assert_eq!(archive.file_length("f1").unwrap(), None);
    assert!(archive.open_file("f1").unwrap().is_none());
}

#[test]
fn boards_store_shapes_and_delete_with_them() {
    let (_dir, archive) = archive();
    let board = ArchivedBoard {
        id: "b1".into(),
        name: "Tafel".into(),
        created_by: ALICE.into(),
        created_at: T0,
    };
    archive.save_board(&board).unwrap();
    archive
        .change_board(
            "b1",
            &[
                ("s1".into(), Some("{}".into())),
                ("s2".into(), Some("{\"a\":1}".into())),
            ],
        )
        .unwrap();
    archive.change_board("b1", &[("s1".into(), None)]).unwrap();

    let shapes = archive.board_shapes("b1").unwrap();
    assert_eq!(shapes.len(), 1);
    assert_eq!(shapes[0].json, "{\"a\":1}");
    assert_eq!(archive.boards().unwrap(), [board]);
    assert!(
        archive
            .change_board("b1", &[("../x".into(), None)])
            .is_err()
    );

    archive.delete_board("b1").unwrap();
    assert!(archive.boards().unwrap().is_empty());
    assert!(archive.board_shapes("b1").unwrap().is_empty());
}

#[test]
fn refuses_archives_from_a_newer_version() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("neu.sqlar");
    {
        let archive = ChannelArchive::open(&path).unwrap();
        archive
            .sqlar()
            .write(|c| c.execute("PRAGMA user_version=99", &[]))
            .unwrap();
        assert_eq!(archive.path(), path);
    }
    assert!(matches!(
        ChannelArchive::open(&path),
        Err(StorageError::NewerSchema { version: 99, .. })
    ));
}

#[test]
fn continues_the_chat_of_a_csharp_archive() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("1.sqlar");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/csharp/archives/channels/1.sqlar");
    std::fs::copy(fixture, &path).unwrap();

    let archive = ChannelArchive::open(&path).unwrap();
    assert_eq!(archive.last_seq().unwrap(), 51);
    let image = archive.image("img01").unwrap().unwrap();
    assert_eq!((image.width, image.height), (1, 1));
    assert_eq!(archive.files().unwrap().len(), 2);
    assert_eq!(archive.boards().unwrap()[0].name, "Tafel");
    assert_eq!(archive.board_shapes("board01").unwrap().len(), 2);

    let next = archive.append(T0, ALICE, "Rust", "weiter", None).unwrap();
    assert_eq!(next.seq, 52);
    assert_eq!(texts(&archive.before(52, 1).unwrap()), ["Bild"]);
}
