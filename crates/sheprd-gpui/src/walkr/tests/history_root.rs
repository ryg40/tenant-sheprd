use crate::walkr::history::{has_sessions, has_sessions_within};
use std::{fs, path::Path};

const SESSION: &str = "20261101T120000Z-0123456789ab";

fn sessions(home: &Path) -> std::path::PathBuf {
    home.join(".local/.walkr/compare/sessions")
}

#[test]
fn a_session_directory_with_a_valid_name_is_found() {
    let home = tempfile::tempdir().unwrap();
    assert!(!has_sessions(home.path()));
    fs::create_dir_all(sessions(home.path()).join(SESSION)).unwrap();
    assert!(has_sessions(home.path()));
}

#[cfg(unix)]
#[test]
fn the_check_never_follows_a_symlink() {
    use std::os::unix::fs::symlink;
    let elsewhere = tempfile::tempdir().unwrap();
    fs::create_dir(elsewhere.path().join(SESSION)).unwrap();
    // The sessions folder is a symlink to a folder with a valid entry.
    let home = tempfile::tempdir().unwrap();
    let root = sessions(home.path());
    fs::create_dir_all(root.parent().unwrap()).unwrap();
    symlink(elsewhere.path(), &root).unwrap();
    assert!(!has_sessions(home.path()));
    // An entry that is a symlink to a directory.
    let home = tempfile::tempdir().unwrap();
    let root = sessions(home.path());
    fs::create_dir_all(&root).unwrap();
    symlink(elsewhere.path().join(SESSION), root.join(SESSION)).unwrap();
    assert!(!has_sessions(home.path()));
}

#[test]
fn the_check_stops_at_the_entry_bound() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(sessions(home.path()).join(SESSION)).unwrap();
    assert!(!has_sessions_within(home.path(), 0));
    assert!(has_sessions_within(home.path(), 1));
}

#[test]
fn other_names_and_regular_files_are_not_counted() {
    let home = tempfile::tempdir().unwrap();
    let root = sessions(home.path());
    fs::create_dir_all(root.join("x")).unwrap();
    for name in [
        "..x",
        "20261101T120000Z-0123456789ab.bak",
        "20261101T120000Z-0123456789AB",
        "20261101T120000Z-0123456789a",
    ] {
        fs::create_dir(root.join(name)).unwrap();
    }
    fs::write(root.join(SESSION), b"").unwrap();
    assert!(!has_sessions(home.path()));
}
