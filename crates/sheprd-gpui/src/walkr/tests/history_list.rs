use crate::walkr::history::{Deadline, RowState, list, row};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const SESSION: &str = "20261101T120000Z-0123456789ab";

/// A reader that counts the bytes it gives.
pub(super) struct Counting<R> {
    pub(super) inner: R,
    pub(super) read: u64,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buf)?;
        self.read += read as u64;
        Ok(read)
    }
}

pub(super) fn record(id: &str) -> String {
    format!(
        r#"{{"schema":"walkr-compare-session","version":1,"sessionId":"{id}","createdAt":"2026-11-01T12:00:00.000Z","updatedAt":"2026-11-01T12:00:00.000Z","revision":1,"status":"open","createAction":{{"actionId":"a"}},"workflow":"ui","title":"Invented title","brief":"Invented brief","options":[]}}"#
    )
}

pub(super) fn sessions(home: &Path) -> PathBuf {
    home.join(".local/.walkr/compare/sessions")
}

fn later() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

#[test]
fn a_row_reads_at_most_the_prefix_of_a_record() {
    // A record that never ends: the row stops at its bound.
    let mut endless = Counting {
        inner: io::repeat(b' '),
        read: 0,
    };
    let _ = row(SESSION, &mut endless);
    assert!(endless.read <= crate::walkr::history::PREFIX);
    let whole = record(SESSION);
    let mut bounded = Counting {
        inner: Deadline::new(whole.as_bytes(), later()),
        read: 0,
    };
    assert!(matches!(
        row(SESSION, &mut bounded).state,
        RowState::Read { .. }
    ));
}

#[test]
fn a_cut_or_reordered_prefix_gives_an_unreadable_or_partial_row() {
    let whole = record(SESSION);
    // Each cut, also one inside a string or an escape, must not panic.
    for end in 0..whole.len() {
        let row = row(SESSION, &whole.as_bytes()[..end]);
        match row.state {
            RowState::Read { title, .. } => {
                assert!(title.is_none_or(|title| "Invented title".starts_with(&title)));
            }
            RowState::Unreadable => {}
            RowState::TimedOut => panic!("no time limit was set"),
        }
    }
    // The fields of a row after the rest: still a row, or unreadable.
    let reordered = format!(
        r#"{{"brief":"b","title":"T","options":[],"status":"submitted","sessionId":"{SESSION}","schema":"walkr-compare-session","version":1}}"#
    );
    assert!(matches!(
        row(SESSION, reordered.as_bytes()).state,
        RowState::Read { .. }
    ));
    // Another schema, or another session ID, is unreadable.
    let other = whole.replace("walkr-compare-session", "other");
    assert_eq!(row(SESSION, other.as_bytes()).state, RowState::Unreadable);
    let moved = record("20261101T120000Z-ffffffffffff");
    assert_eq!(row(SESSION, moved.as_bytes()).state, RowState::Unreadable);
    // A field of another type gives no row text.
    let typed = whole.replace(r#""title":"Invented title""#, r#""title":7"#);
    assert!(!matches!(
        row(SESSION, typed.as_bytes()).state,
        RowState::Read { title: Some(_), .. }
    ));
}

#[test]
fn another_record_version_is_unreadable() {
    let future = record(SESSION).replace(r#""version":1"#, r#""version":2"#);
    assert_eq!(row(SESSION, future.as_bytes()).state, RowState::Unreadable);
}

#[cfg(unix)]
#[test]
fn the_list_opens_no_record_outside_its_root() {
    use std::os::unix::fs::symlink;
    let outside = tempfile::tempdir().unwrap();
    let folder = outside.path().join(SESSION);
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("session.json"), record(SESSION)).unwrap();
    // A symlinked session folder is not listed.
    let home = tempfile::tempdir().unwrap();
    let root = sessions(home.path());
    fs::create_dir_all(&root).unwrap();
    symlink(&folder, root.join(SESSION)).unwrap();
    assert!(list(home.path(), later()).is_empty());
    // A symlinked record is not read.
    let home = tempfile::tempdir().unwrap();
    let root = sessions(home.path());
    fs::create_dir_all(root.join(SESSION)).unwrap();
    symlink(
        folder.join("session.json"),
        root.join(SESSION).join("session.json"),
    )
    .unwrap();
    let rows = list(home.path(), later());
    assert!(rows.iter().all(|row| row.state == RowState::Unreadable));
    // A symlinked sessions folder lists nothing.
    let home = tempfile::tempdir().unwrap();
    let root = sessions(home.path());
    fs::create_dir_all(root.parent().unwrap()).unwrap();
    symlink(outside.path(), &root).unwrap();
    assert!(list(home.path(), later()).is_empty());
}
