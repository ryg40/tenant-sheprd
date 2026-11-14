//! The saved sessions that wait for a pick, for the count on the activity
//! bar: the tally of a host's rows, and the read of this computer. A record
//! gives `status`, `createdAt` and `title` from its first bytes, as a row of
//! the list does, and the newest waiting one also `repo.name`. Records are
//! untrusted data: a symlink, a record over the size bound or a malformed
//! one is skipped, and the others still count. An SSH host runs the reader
//! of `waiting::remote`.
use super::{
    list::{Row, RowState, Status, row},
    open::{self, Deadline},
};
use crate::walkr::{run::is_session_id, waiting::Error};
use chrono::{DateTime, Utc};
use serde::de::{self, DeserializeSeed, IgnoredAny, MapAccess, Visitor};
use std::{fmt, fs, io::Read, path::Path, time::Instant};

/// The most bytes read to find `repo.name`, which Walkr writes after the
/// brief of at most 8,000 characters.
pub(crate) const REPO_PREFIX: u64 = 64 * 1024;
/// The most waiting sessions a host keeps for the section; the count
/// covers every one read.
pub(crate) const KEPT: usize = super::list::ROWS;
/// The longest repository name Walkr accepts.
const REPO_NAME: usize = 100;

/// One session that waits for a pick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pending {
    pub(crate) id: String,
    pub(crate) created: Option<DateTime<Utc>>,
    pub(crate) title: Option<String>,
}

/// What one host's sessions say.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tally {
    /// The sessions that wait for a pick.
    pub(crate) waiting: usize,
    /// The newest [`KEPT`] of them, newest first.
    pub(crate) newest: Vec<Pending>,
    /// The `repo.name` of the newest one, when its record names one.
    pub(crate) repo: Option<String>,
    /// The session records read, whatever their status.
    pub(crate) sessions: usize,
}

/// The tally of `rows`, which counts only the `open` ones.
pub(crate) fn tally(rows: impl IntoIterator<Item = Row>) -> Tally {
    let mut sessions = 0;
    let mut newest: Vec<Pending> = rows
        .into_iter()
        .filter_map(|row| {
            let RowState::Read { title, status, .. } = row.state else {
                return None;
            };
            sessions += 1;
            (status == Some(Status::Open)).then_some(Pending {
                id: row.id,
                created: row.created,
                title,
            })
        })
        .collect();
    newest.sort_unstable_by(|a, b| (b.created, &b.id).cmp(&(a.created, &a.id)));
    let waiting = newest.len();
    newest.truncate(KEPT);
    Tally {
        waiting,
        newest,
        repo: None,
        sessions,
    }
}

/// The waiting sessions of this computer under `home`. At most
/// [`super::ENTRY_LIMIT`] entries are read; each record opens as a row of
/// the list does. No sessions folder counts 0; a folder that cannot be
/// read, or a read past `deadline`, fails the whole host, so that it keeps
/// its last count.
pub(crate) fn local(home: &Path, deadline: Instant) -> Result<Tally, Error> {
    let root = super::root(home);
    match fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Tally::default());
        }
        Err(error) => return Err(Error::Read(error)),
        // A symlink in place of the folder is refused, as the list does:
        // `symlink_metadata` does not follow it.
        Ok(meta) if !meta.is_dir() => return Ok(Tally::default()),
        Ok(_) => {}
    }
    let mut ids = Vec::new();
    for entry in fs::read_dir(&root)
        .map_err(Error::Read)?
        .take(super::ENTRY_LIMIT)
    {
        ids.push(entry.map_err(Error::Read)?);
    }
    let ids: Vec<String> = ids
        .into_iter()
        // `DirEntry::file_type` does not follow a symlink.
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_session_id(name))
        .collect();
    let mut rows = Vec::with_capacity(ids.len());
    for id in ids {
        if Instant::now() >= deadline {
            return Err(Error::TimedOut);
        }
        let Ok((file, _)) = open::record(home, &id) else {
            continue;
        };
        let read = row(&id, Deadline::new(file, deadline));
        if read.state == RowState::TimedOut {
            return Err(Error::TimedOut);
        }
        rows.push(read);
    }
    let mut tally = tally(rows);
    if let Some(newest) = tally.newest.first() {
        tally.repo = open::record(home, &newest.id).ok().and_then(|(file, _)| {
            let mut prefix = Vec::new();
            Deadline::new(file, deadline)
                .take(REPO_PREFIX)
                .read_to_end(&mut prefix)
                .ok()?;
            repo_name(&prefix)
        });
    }
    Ok(tally)
}

/// `repo.name` from the first bytes of a record, when it is a name Walkr
/// accepts; see [`valid_repo`].
pub(crate) fn repo_name(prefix: &[u8]) -> Option<String> {
    let mut repo = Repo::default();
    // The prefix may end inside the record: what came before counts.
    let _ = (&mut repo).deserialize(&mut serde_json::Deserializer::from_slice(prefix));
    repo.name.filter(|name| valid_repo(name))
}

/// Whether `name` is a repository name Walkr accepts: 1 to 100 letters,
/// digits, dots, underscores or hyphens.
pub(crate) fn valid_repo(name: &str) -> bool {
    (1..=REPO_NAME).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[derive(Default)]
struct Repo {
    name: Option<String>,
}

impl<'de> DeserializeSeed<'de> for &mut Repo {
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for &mut Repo {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a Walkr Compare record")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        #[derive(serde::Deserialize)]
        struct Named {
            name: Option<String>,
        }
        while let Some(key) = map.next_key::<std::borrow::Cow<'de, str>>()? {
            if key == "repo" {
                self.name = map
                    .next_value::<Option<Named>>()
                    .ok()
                    .flatten()
                    .and_then(|named| named.name);
                // Stop here: the rest of the record is not needed.
                return Err(de::Error::custom("found"));
            }
            map.next_value::<IgnoredAny>()?;
        }
        Ok(())
    }
}
