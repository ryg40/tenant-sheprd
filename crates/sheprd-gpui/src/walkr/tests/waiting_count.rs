//! The waiting count takes only `open` sessions. A symlink, a record over
//! the size bound and a malformed record are skipped, and the host still
//! answers. The fixture serves the host reader too (`waiting_remote`).
use crate::walkr::history::pending::{self, Tally};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const OPEN: &str = "20261103T120000Z-0123456789ab";
const OLDER_OPEN: &str = "20261101T120000Z-0123456789ab";
const SUBMITTED: &str = "20261102T120000Z-0123456789ab";
const MALFORMED: &str = "20261104T120000Z-0123456789ab";
const OVERSIZED: &str = "20261105T120000Z-0123456789ab";
const LINKED: &str = "20261106T120000Z-0123456789ab";

fn root(home: &Path) -> PathBuf {
    home.join(".local/.walkr/compare/sessions")
}

pub(super) fn record(id: &str, status: &str, repo: &str) -> String {
    format!(
        r#"{{"schema":"walkr-compare-session","version":1,"sessionId":"{id}","createdAt":"{created}","updatedAt":"{created}","revision":1,"status":"{status}","createAction":{{"actionId":"a1","requestSha256":"00","at":"{created}"}},"workflow":"pick","title":"Title {id}","brief":"A brief","repo":{{"name":"{repo}"}},"clarifications":[],"options":[]}}"#,
        created = format_args!(
            "{}-{}-{}T{}:{}:{}Z",
            &id[0..4],
            &id[4..6],
            &id[6..8],
            &id[9..11],
            &id[11..13],
            &id[13..15]
        ),
    )
}

pub(super) fn write(home: &Path, id: &str, bytes: &[u8]) -> PathBuf {
    let dir = root(home).join(id);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("session.json");
    fs::write(&path, bytes).unwrap();
    path
}

/// Two open sessions, one submitted, one malformed and one oversized open
/// record, and, on Unix, a symlink to an open session of another folder.
pub(super) fn home() -> (tempfile::TempDir, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    let path = home.path();
    write(path, OPEN, record(OPEN, "open", "valuetainme").as_bytes());
    write(
        path,
        OLDER_OPEN,
        record(OLDER_OPEN, "open", "other").as_bytes(),
    );
    write(
        path,
        SUBMITTED,
        record(SUBMITTED, "submitted", "x").as_bytes(),
    );
    write(
        path,
        MALFORMED,
        b"{\"schema\":\"walkr-compare-session\",\"version\":",
    );
    let oversized = write(path, OVERSIZED, record(OVERSIZED, "open", "x").as_bytes());
    let file = fs::OpenOptions::new().write(true).open(oversized).unwrap();
    file.set_len(16 * 1024 * 1024 + 1).unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        let target = write(
            elsewhere.path(),
            LINKED,
            record(LINKED, "open", "x").as_bytes(),
        );
        std::os::unix::fs::symlink(target.parent().unwrap(), root(path).join(LINKED)).unwrap();
    }
    (home, elsewhere)
}

pub(super) fn assert_counts_open_only(tally: &Tally) {
    assert_eq!(tally.waiting, 2, "{tally:?}");
    let ids: Vec<&str> = tally
        .newest
        .iter()
        .map(|pending| pending.id.as_str())
        .collect();
    assert_eq!(ids, [OPEN, OLDER_OPEN]);
    assert_eq!(tally.newest[0].repo.as_deref(), Some("valuetainme"));
}

#[test]
fn this_computer_counts_open_sessions_and_skips_refused_records() {
    let (home, _elsewhere) = home();
    let tally = pending::local(home.path(), Instant::now() + Duration::from_secs(5)).unwrap();
    assert_counts_open_only(&tally);
}
