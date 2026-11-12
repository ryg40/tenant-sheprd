#![allow(clippy::unwrap_used)]
use super::*;
use std::fs;

mod listing;
mod names;
mod roots;

fn not_cancelled() -> AtomicBool {
    AtomicBool::new(false)
}

fn names_of(listing: &Listing) -> Vec<&str> {
    listing
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect()
}

/// A folder to list, and beside it a folder that is outside the root.
fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let outside = dir.path().join("outside");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
    fs::write(root.join("README.md"), "# Read me").unwrap();
    fs::write(outside.join("secret.txt"), "secret").unwrap();
    (dir, root, outside)
}
