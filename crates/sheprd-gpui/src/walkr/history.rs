//! The saved Walkr Compare sessions: the list of this computer ([`list`]),
//! one session ([`detail`]), and the ones that wait for a pick on any
//! counted host ([`pending`]).
//! Only worker threads call them. Session files are untrusted data: the
//! reader bounds each read, refuses symlinks, and keeps text only.
mod collections;
mod detail;
mod error;
mod list;
mod open;
pub(crate) mod pending;
mod text;

pub(crate) use detail::{Detail, OptionBody, RATINGS_PAGE, detail};
pub(crate) use error::Error;
pub(crate) use list::{Row, RowState, Status, list, read_row};
#[cfg(test)]
pub(super) use {
    detail::parse,
    list::{PREFIX, row},
    open::Deadline,
};

#[cfg(all(test, unix))]
pub(super) use open::open_record;

use std::path::{Path, PathBuf};

/// The most directory entries one check reads. A folder with more names
/// than this answers from the ones read.
pub(crate) const ENTRY_LIMIT: usize = 5_000;

/// Walkr's default session root under the home.
const SESSIONS: [&str; 4] = [".local", ".walkr", "compare", "sessions"];

/// `<home>/.local/.walkr/compare/sessions`. `HUMAN_REVIEW_COMPARE_ROOT` is
/// not read.
pub(crate) fn root(home: &Path) -> PathBuf {
    SESSIONS
        .iter()
        .fold(home.to_path_buf(), |path, part| path.join(part))
}
