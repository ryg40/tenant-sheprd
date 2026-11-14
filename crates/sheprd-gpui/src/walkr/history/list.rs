//! The newest saved sessions, each from the first bytes of its record.
//!
//! Walkr writes the keys of a record in the order `schema, version,
//! sessionId, createdAt, updatedAt, revision, status, createAction,
//! workflow, title, brief, …`, so the fields of a row come first. A visitor
//! takes them from a prefix and stops when it has them or the bytes end.
use super::{
    open::{self, Deadline},
    text,
};
use crate::walkr::run::is_session_id;
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::de::{self, DeserializeSeed, IgnoredAny, MapAccess, Visitor};
use std::{fmt, fs, io::Read, path::Path, time::Instant};

/// The most rows of a list: the newest names.
pub(crate) const ROWS: usize = 100;
/// The most bytes of a record that a row reads.
pub(crate) const PREFIX: u64 = 8 * 1024;
/// The value of `schema` in a Compare record.
pub(super) const SCHEMA: &str = "walkr-compare-session";

/// A session's state, as its record says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    /// Waiting for a pick.
    Open,
    /// Picked.
    Submitted,
    /// Another value: shown as neither.
    Other,
}

impl From<&str> for Status {
    fn from(text: &str) -> Self {
        match text {
            "open" => Self::Open,
            "submitted" => Self::Submitted,
            _ => Self::Other,
        }
    }
}

/// One saved session in the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    /// The folder name, a session ID.
    pub(crate) id: String,
    /// When it was made: `createdAt`, else the time in its name.
    pub(crate) created: Option<DateTime<Utc>>,
    pub(crate) state: RowState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RowState {
    /// The prefix names this session. A field the prefix lacks is `None`.
    Read {
        title: Option<String>,
        status: Option<Status>,
        workflow: Option<String>,
    },
    /// No record, another `schema`, another `sessionId`, or a refused file.
    Unreadable,
    /// The list's time limit passed before this record was read.
    TimedOut,
}

/// The newest [`ROWS`] sessions under `<home>/.local/.walkr/compare/sessions`,
/// newest first. At most [`super::ENTRY_LIMIT`] entries of the folder are
/// read; a symlink and a name that is not a session ID are not listed.
pub(crate) fn list(home: &Path, deadline: Instant) -> Vec<Row> {
    let root = super::root(home);
    if !open::is_dir(&root) {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .take(super::ENTRY_LIMIT)
        .flatten()
        // `DirEntry::file_type` does not follow a symlink.
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_session_id(name))
        .collect();
    ids.sort_unstable_by(|a, b| b.cmp(a));
    ids.truncate(ROWS);
    ids.into_iter()
        .map(|id| {
            if Instant::now() >= deadline {
                return Row::new(id, RowState::TimedOut);
            }
            match open::record(home, &id) {
                Ok((file, _)) => row(&id, Deadline::new(file, deadline)),
                Err(_) => Row::new(id, RowState::Unreadable),
            }
        })
        .collect()
}

/// The row of session `id` from at most [`PREFIX`] bytes of `record`.
pub(crate) fn row(id: &str, record: impl Read) -> Row {
    let mut prefix = Vec::new();
    if let Err(error) = record.take(PREFIX).read_to_end(&mut prefix) {
        let state = if error.kind() == std::io::ErrorKind::TimedOut {
            RowState::TimedOut
        } else {
            RowState::Unreadable
        };
        return Row::new(id.to_owned(), state);
    }
    let mut fields = Fields::default();
    // The prefix usually ends inside the record: the error that follows is
    // expected, and what was taken before it counts.
    let _ = (&mut fields).deserialize(&mut serde_json::Deserializer::from_slice(&prefix));
    let named = fields.schema.as_deref() == Some(SCHEMA)
        && fields.version == Some(1)
        && fields.session.as_deref() == Some(id);
    if !named {
        return Row::new(id.to_owned(), RowState::Unreadable);
    }
    read_row(
        id,
        fields.created.as_deref(),
        fields.status.as_deref(),
        fields.title.as_deref(),
        fields.workflow.as_deref(),
    )
}

/// The row of session `id` from the fields its record names, which the
/// caller checked to be a Compare record of this ID. Text is cut and
/// cleaned here, as for every row.
pub(crate) fn read_row(
    id: &str,
    created: Option<&str>,
    status: Option<&str>,
    title: Option<&str>,
    workflow: Option<&str>,
) -> Row {
    Row {
        created: created.and_then(parse_time).or_else(|| name_time(id)),
        id: id.to_owned(),
        state: RowState::Read {
            title: title.map(|title| text::chars(title, text::TITLE)),
            status: status.map(Status::from),
            workflow: workflow.map(|workflow| text::chars(workflow, text::TITLE)),
        },
    }
}

impl Row {
    fn new(id: String, state: RowState) -> Self {
        Self {
            created: name_time(&id),
            id,
            state,
        }
    }
}

/// An ISO time of a record.
pub(super) fn parse_time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.with_timezone(&Utc))
}

/// The time in a session ID: `YYYYMMDDTHHMMSSZ-…`, in UTC.
pub(super) fn name_time(id: &str) -> Option<DateTime<Utc>> {
    let stamp = id.get(..15)?;
    NaiveDateTime::parse_from_str(stamp, "%Y%m%dT%H%M%S")
        .ok()
        .map(|time| time.and_utc())
}

/// The fields of a row that a prefix gave.
#[derive(Default)]
struct Fields {
    schema: Option<String>,
    version: Option<u64>,
    session: Option<String>,
    created: Option<String>,
    status: Option<String>,
    workflow: Option<String>,
    title: Option<String>,
}

impl Fields {
    fn complete(&self) -> bool {
        self.schema.is_some()
            && self.version.is_some()
            && self.session.is_some()
            && self.created.is_some()
            && self.status.is_some()
            && self.workflow.is_some()
            && self.title.is_some()
    }

    fn slot(&mut self, key: &str) -> Option<&mut Option<String>> {
        match key {
            "schema" => Some(&mut self.schema),
            "sessionId" => Some(&mut self.session),
            "createdAt" => Some(&mut self.created),
            "status" => Some(&mut self.status),
            "workflow" => Some(&mut self.workflow),
            "title" => Some(&mut self.title),
            _ => None,
        }
    }
}

// The seed fills `Fields` in place, so a record cut inside a value keeps
// the fields before it.
impl<'de> DeserializeSeed<'de> for &mut Fields {
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for &mut Fields {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a Walkr Compare record")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<std::borrow::Cow<'de, str>>()? {
            if key == "version" && self.version.is_none() {
                self.version = map.next_value()?;
                continue;
            }
            match self.slot(&key) {
                // A later copy of a key does not replace the first.
                Some(slot) if slot.is_none() => {
                    // A field of another type leaves its slot empty.
                    *slot = map.next_value::<Option<String>>().ok().flatten();
                    if slot.is_none() {
                        return Err(de::Error::custom("a field of another type"));
                    }
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
            if self.complete() {
                // Stop here: the rest of the prefix is not needed.
                return Err(de::Error::custom("complete"));
            }
        }
        Ok(())
    }
}
