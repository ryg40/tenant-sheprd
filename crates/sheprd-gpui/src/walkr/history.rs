//! The saved Walkr Compare sessions of this computer: whether there is one
//! ([`has_sessions`]), the list ([`list`]) and one session ([`detail`]).
//! Only worker threads call them. Session files are untrusted data: the
//! reader bounds each read, refuses symlinks, and keeps text only.
mod collections;
mod detail;
mod error;
mod list;
mod open;
mod text;

pub(crate) use detail::{Detail, OptionBody, RATINGS_PAGE, detail};
pub(crate) use error::Error;
pub(crate) use list::{Row, RowState, Status, list};
#[cfg(test)]
pub(super) use {
    detail::parse,
    list::{PREFIX, row},
    open::Deadline,
};

#[cfg(all(test, unix))]
pub(super) use open::open_record;

use std::{
    fs,
    path::{Path, PathBuf},
};

/// The most directory entries one check reads. A folder with more names
/// than this answers from the ones read.
pub(crate) const ENTRY_LIMIT: usize = 5_000;

/// Walkr's default session root under the home.
const SESSIONS: [&str; 4] = [".local", ".walkr", "compare", "sessions"];

/// Whether `<home>/.local/.walkr/compare/sessions` holds a directory whose
/// name is a session ID. A symlink, as the folder or as an entry, does not
/// count, and no file is opened.
///
/// The checks go by path, so a process of this user that swaps the folder
/// for a symlink between them can make the answer true. That shows the icon
/// and nothing more: without no-follow directory handles, which `std` does
/// not offer, the answer stays a hint, and no reader may trust it.
pub(crate) fn has_sessions(home: &Path) -> bool {
    has_sessions_within(home, ENTRY_LIMIT)
}

/// `<home>/.local/.walkr/compare/sessions`. `HUMAN_REVIEW_COMPARE_ROOT` is
/// not read.
pub(crate) fn root(home: &Path) -> PathBuf {
    SESSIONS
        .iter()
        .fold(home.to_path_buf(), |path, part| path.join(part))
}

pub(super) fn has_sessions_within(home: &Path, limit: usize) -> bool {
    let root = root(home);
    if !fs::symlink_metadata(&root).is_ok_and(|meta| meta.file_type().is_dir()) {
        return false;
    }
    let Ok(entries) = fs::read_dir(&root) else {
        return false;
    };
    entries.take(limit).flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(super::run::is_session_id)
            && fs::symlink_metadata(entry.path()).is_ok_and(|meta| meta.file_type().is_dir())
    })
}
