//! The Walkr sessions cleared out of the waiting count are remembered with
//! the rest of the chrome: host and session ID only.
use super::*;
use crate::{automations::Machine, walkr::waiting::Dismissed};

const OPEN: &str = "20261103T120000Z-0123456789ab";
const OTHER: &str = "20261104T120000Z-0123456789ab";

#[core::prelude::v1::test]
fn walkr_dismissed_sessions_survive_a_save_and_a_load() {
    let directory = TestDirectory::new();
    let path = directory.0.join("preferences.json");
    let mut dismissed = Dismissed::default();
    dismissed.insert(&Machine::Local, [OPEN]);
    dismissed.insert(&Machine::Ssh("builder.example".into()), [OPEN, OTHER]);
    let chrome = Chrome {
        sidebar_width: Some(250.),
        walkr_dismissed: dismissed,
        ..Chrome::default()
    };
    let mut preferences = Preferences::start(Ok(path.clone()));
    await_loaded(&mut preferences);
    preferences.save(chrome.clone());
    let worker = preferences.worker.take().unwrap();
    drop(preferences);
    worker.join().unwrap();
    let mut preferences = Preferences::start(Ok(path.clone()));
    assert_eq!(await_loaded(&mut preferences), chrome);
    // The file names no SSH target, only its hash.
    let text = fs::read_to_string(&path).unwrap();
    assert!(!text.contains("builder"), "{text}");
    // A damaged host or ID is forgotten, and the rest of the file stays.
    fs::write(
        &path,
        format!(
            r#"{{"sidebar_width_px": 250, "walkr_dismissed": {{"local": ["{OPEN}", "../x", 3], "ssh-builder": ["{OPEN}"], "ssh-00": 1}}}}"#
        ),
    )
    .unwrap();
    let read = read_chrome(&path).unwrap();
    let mut expected = Dismissed::default();
    expected.insert(&Machine::Local, [OPEN]);
    assert_eq!(read.walkr_dismissed, expected);
    assert_eq!(read.sidebar_width, Some(250.));
}

#[core::prelude::v1::test]
fn a_save_keeps_the_walkr_dismissals_another_window_stored() {
    let directory = TestDirectory::new();
    let path = directory.0.join("preferences.json");
    let mut preferences = Preferences::start(Ok(path.clone()));
    await_loaded(&mut preferences);
    // Another window of this endpoint stores its dismissal after the load.
    let mut theirs = Dismissed::default();
    theirs.insert(&Machine::Local, [OTHER]);
    write_chrome(
        &path,
        Chrome {
            walkr_dismissed: theirs,
            ..Chrome::default()
        },
    )
    .unwrap();
    let mut mine = Dismissed::default();
    mine.insert(&Machine::Local, [OPEN]);
    preferences.save(Chrome {
        walkr_dismissed: mine,
        ..Chrome::default()
    });
    let worker = preferences.worker.take().unwrap();
    drop(preferences);
    worker.join().unwrap();
    let mut both = Dismissed::default();
    both.insert(&Machine::Local, [OPEN, OTHER]);
    assert_eq!(read_chrome(&path).unwrap().walkr_dismissed, both);
}
