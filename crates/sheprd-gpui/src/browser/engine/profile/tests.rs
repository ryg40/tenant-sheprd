#![allow(clippy::unwrap_used)]
use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

const OLD: &str = "cookies of before the clearing";

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
}

/// A folder as Servo leaves it, with a link out of it, and a file outside
/// that the link names.
fn kept(root: &Path) -> (PathBuf, PathBuf) {
    let dir = root.join("data");
    let outside = root.join("outside");
    fs::create_dir_all(dir.join("webstorage").join("origin")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("keep.txt"), "mine").unwrap();
    fs::write(dir.join(COOKIES), OLD).unwrap();
    fs::write(dir.join("auth_cache.json"), "{}").unwrap();
    fs::write(dir.join("webstorage").join("origin").join("db"), "x").unwrap();
    symlink(&outside, dir.join("link")).unwrap();
    (dir, outside)
}

/// A process whose engine has `dir` open.
fn running(dir: &Path) -> Hold {
    let mut hold = Hold::default();
    hold.open(dir).unwrap();
    hold
}

fn is_empty(dir: &Path) -> bool {
    fs::read_dir(dir).unwrap().next().is_none()
}

#[test]
fn clearing_empties_the_folder_and_nothing_outside_it() {
    let root = tempfile::tempdir().unwrap();
    let (dir, outside) = kept(root.path());
    assert_eq!(Hold::default().clear(&dir).unwrap(), Cleared::Now);
    assert!(is_empty(&dir));
    // The link went; what it named did not.
    assert_eq!(
        fs::read_to_string(outside.join("keep.txt")).unwrap(),
        "mine"
    );
}

/// The defect this guards: the cookies of before a clearing came back when
/// the run was killed before Servo wrote its cookie file again.
#[test]
fn cleared_cookies_do_not_come_back_after_a_run_that_is_killed() {
    let root = tempfile::tempdir().unwrap();
    let (dir, outside) = kept(root.path());
    let run = running(&dir);
    assert_eq!(run.clear(&dir).unwrap(), Cleared::AtNextStart);
    // Off the disk the moment the user confirms, whatever ends the run.
    assert!(!dir.join(COOKIES).exists());
    // Servo still has its storage open: nothing is removed under it.
    assert!(dir.join("webstorage").join("origin").join("db").exists());
    // The engine drops its cookies, and the run is killed before Servo
    // writes anything: the lock goes with the process.
    keep_cookies_in(&dir).unwrap();
    drop(run);
    let next = running(&dir);
    assert!(is_empty(&dir));
    assert!(outside.join("keep.txt").exists());
    drop(next);
    // The same when the engine never answered.
    fs::write(dir.join(COOKIES), OLD).unwrap();
    let run = running(&dir);
    run.clear(&dir).unwrap();
    drop(run);
    drop(running(&dir));
    assert!(is_empty(&dir));
}

#[test]
fn a_sign_in_made_after_a_clearing_stays_and_the_rest_goes_at_quit() {
    let root = tempfile::tempdir().unwrap();
    let (dir, _) = kept(root.path());
    let mut run = running(&dir);
    run.clear(&dir).unwrap();
    keep_cookies_in(&dir).unwrap();
    // Servo shuts down: it writes the cookies set since the clearing, and
    // the list of sites it still held in memory.
    fs::write(dir.join(COOKIES), "new").unwrap();
    fs::write(dir.join("hsts_list.json"), "old sites").unwrap();
    run.close(&dir).unwrap();
    assert_eq!(fs::read_to_string(dir.join(COOKIES)).unwrap(), "new");
    assert!(!dir.join("hsts_list.json").exists());
    assert!(!dir.join("webstorage").exists());
    // The clearing ran once: the next run's files stay.
    drop(running(&dir));
    assert_eq!(fs::read_to_string(dir.join(COOKIES)).unwrap(), "new");
    // An engine that did not drop its cookies wrote the old ones again:
    // they go with the rest.
    let mut run = running(&dir);
    run.clear(&dir).unwrap();
    fs::write(dir.join(COOKIES), OLD).unwrap();
    run.close(&dir).unwrap();
    assert!(is_empty(&dir));
    // A mark that is not the app's own word keeps nothing either.
    fs::write(dir.join(COOKIES), OLD).unwrap();
    fs::write(dir.join(PENDING), "keep-cookies\n").unwrap();
    drop(running(&dir));
    assert!(is_empty(&dir));
    // Without a clearing there is nothing to record.
    keep_cookies_in(&dir).unwrap();
    assert!(is_empty(&dir));
}

#[test]
fn a_second_process_neither_shares_the_folder_nor_empties_it_under_the_first() {
    let root = tempfile::tempdir().unwrap();
    let (dir, _) = kept(root.path());
    let first = running(&dir);
    // Its engine keeps nothing between runs.
    let mut second = Hold::default();
    assert!(matches!(second.open(&dir), Err(Error::InUse)));
    // Its clearing leaves the first engine's open files alone.
    assert_eq!(second.clear(&dir).unwrap(), Cleared::AtNextStart);
    assert!(dir.join("webstorage").join("origin").join("db").exists());
    assert!(!dir.join(COOKIES).exists());
    // And a start of it does not run the clearing under the first.
    assert!(matches!(second.open(&dir), Err(Error::InUse)));
    assert!(dir.join("webstorage").exists());
    drop(first);
    second.open(&dir).unwrap();
    assert!(is_empty(&dir));
}

#[test]
fn the_folder_and_the_files_in_it_are_closed_to_others() {
    let root = tempfile::tempdir().unwrap();
    let (dir, _) = kept(root.path());
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(dir.join(COOKIES), fs::Permissions::from_mode(0o644)).unwrap();
    drop(running(&dir));
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&dir.join(COOKIES)), 0o600);
    // A new folder is made private as well.
    let new = root.path().join("new");
    drop(running(&new));
    assert_eq!(mode(&new), 0o700);
}

#[test]
fn a_link_in_place_of_the_folder_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let (dir, outside) = kept(root.path());
    let link = root.path().join("linked");
    symlink(&outside, &link).unwrap();
    assert!(matches!(Hold::default().open(&link), Err(Error::NotOwn)));
    assert!(matches!(Hold::default().clear(&link), Err(Error::NotOwn)));
    assert!(outside.join("keep.txt").exists());
    // A folder that was never made has nothing to clear.
    let none = root.path().join("none");
    assert_eq!(Hold::default().clear(&none).unwrap(), Cleared::Now);
    assert!(dir.join(COOKIES).exists());
}
