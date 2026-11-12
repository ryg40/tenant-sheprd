//! One saved session, as text only. The `checksum` of a record is not
//! checked: the reader shows what the file holds and changes nothing.
use super::{
    Error,
    collections::{self, First, Newest, NewestSeed},
    list::{SCHEMA, Status, name_time, parse_time},
    open::{self, Deadline},
    text,
};
use crate::walkr::run::is_session_id;
use chrono::{DateTime, Utc};
use serde::{
    Deserialize,
    de::{DeserializeSeed, IgnoredAny, MapAccess, Visitor},
};
use std::{fmt, io::Read, marker::PhantomData, path::Path, time::Instant};

/// The most bytes of a record that a detail reads.
pub(crate) const LIMIT: u64 = 16 * 1024 * 1024;
/// Ratings are expanded by this many entries per request.
pub(crate) const RATINGS_PAGE: usize = 64;

/// A saved session, cut to the bounds of [`super::text`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Detail {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) workflow: String,
    pub(crate) status: Status,
    pub(crate) created: Option<DateTime<Utc>>,
    pub(crate) brief: String,
    /// Questions with their answers.
    pub(crate) clarifications: Vec<(String, String)>,
    pub(crate) clarifications_omitted: usize,
    pub(crate) options: Vec<Choice>,
    pub(crate) options_omitted: usize,
    /// The owner's pick and words, once submitted.
    pub(crate) result: Option<Pick>,
    /// The agent's report of what it did with the pick.
    pub(crate) outcome: Option<String>,
    /// The newest requested ratings, oldest first.
    pub(crate) ratings: Vec<Rating>,
    pub(crate) ratings_total: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Choice {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) summary: Option<String>,
    pub(crate) body: OptionBody,
}

/// What an option shows. HTML content never leaves the reader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OptionBody {
    /// A `text` or `markdown` option as plain text, and whether it was cut.
    Text { text: String, cut: bool },
    /// An `html` option, or a format this reader does not know: only its
    /// size is kept.
    NotDrawn { bytes: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pick {
    /// The picked option's ID, or `None` for "none of them".
    pub(crate) pick: Option<String>,
    /// Comments by option ID.
    pub(crate) comments: Vec<(String, String)>,
    pub(crate) comments_omitted: usize,
    pub(crate) note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Rating {
    /// Half stars, from 1 to 10 as Walkr writes them.
    pub(crate) value: i64,
    pub(crate) note: Option<String>,
}

/// The session `id` of `<home>/.local/.walkr/compare/sessions`.
pub(crate) fn detail(
    home: &Path,
    id: &str,
    ratings: usize,
    deadline: Instant,
) -> Result<Detail, Error> {
    if !is_session_id(id) {
        return Err(Error::InvalidId);
    }
    let (file, _) = open::record(home, id)?;
    parse(id, Deadline::new(file, deadline), ratings)
}

/// Session `id` from at most [`LIMIT`] and one bytes of `record`. One byte
/// more than the limit tells a record that grew since its size was seen.
pub(crate) fn parse(id: &str, record: impl Read, ratings: usize) -> Result<Detail, Error> {
    let mut bytes = Vec::new();
    record
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(Error::read)?;
    if bytes.len() as u64 > LIMIT {
        return Err(Error::TooLarge);
    }
    let mut decoder = serde_json::Deserializer::from_slice(&bytes);
    let raw = RawSeed(ratings)
        .deserialize(&mut decoder)
        .map_err(Error::Invalid)?;
    decoder.end().map_err(Error::Invalid)?;
    drop(bytes);
    if raw.schema.as_deref() != Some(SCHEMA) {
        return Err(Error::Schema);
    }
    if raw.version != Some(1) {
        return Err(Error::UnknownVersion(raw.version.unwrap_or(0)));
    }
    if raw.session_id.as_deref() != Some(id) {
        return Err(Error::SessionMismatch);
    }
    Ok(raw.into_detail(id))
}

fn field(value: &str) -> String {
    text::chars(value, text::FIELD)
}

fn optional(value: Option<String>) -> Option<String> {
    value.map(|value| field(&value))
}

// Owned and lenient: unknown keys are ignored, and a missing field is empty.
#[derive(Default)]
struct Raw {
    schema: Option<String>,
    version: Option<u64>,
    session_id: Option<String>,
    created_at: Option<String>,
    status: Option<String>,
    workflow: String,
    title: String,
    brief: String,
    clarifications: First<RawClarification>,
    options: First<RawOption>,
    result: Option<RawResult>,
    outcome: Option<RawOutcome>,
    ratings: Newest<RawRating>,
}

struct RawSeed(usize);

impl<'de> DeserializeSeed<'de> for RawSeed {
    type Value = Raw;
    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<Raw, D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for RawSeed {
    type Value = Raw;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a Walkr Compare record")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Raw, A::Error> {
        let mut raw = Raw::default();
        while let Some(key) = map.next_key::<std::borrow::Cow<'de, str>>()? {
            match key.as_ref() {
                "schema" => raw.schema = map.next_value()?,
                "version" => raw.version = map.next_value()?,
                "sessionId" => raw.session_id = map.next_value()?,
                "createdAt" => raw.created_at = map.next_value()?,
                "status" => raw.status = map.next_value()?,
                "workflow" => raw.workflow = map.next_value()?,
                "title" => raw.title = map.next_value()?,
                "brief" => raw.brief = map.next_value()?,
                "clarifications" => raw.clarifications = map.next_value()?,
                "options" => raw.options = map.next_value()?,
                "result" => raw.result = map.next_value()?,
                "outcome" => raw.outcome = map.next_value()?,
                "ratings" => raw.ratings = map.next_value_seed(NewestSeed(self.0, PhantomData))?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(raw)
    }
}

#[derive(Deserialize)]
struct RawClarification {
    #[serde(default)]
    question: String,
    #[serde(default)]
    answer: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOption {
    #[serde(default)]
    option_id: String,
    #[serde(default)]
    label: String,
    summary: Option<String>,
    #[serde(default)]
    format: String,
    #[serde(default)]
    content: String,
}

#[derive(Deserialize)]
struct RawResult {
    pick: Option<String>,
    #[serde(default, deserialize_with = "collections::comments")]
    comments: First<(String, String)>,
    note: Option<String>,
}

#[derive(Deserialize)]
struct RawOutcome {
    #[serde(default)]
    summary: String,
}

#[derive(Deserialize)]
struct RawRating {
    value: i64,
    note: Option<String>,
}

impl Raw {
    fn into_detail(self, id: &str) -> Detail {
        let options = self
            .options
            .items
            .into_iter()
            .map(|option| {
                let body = match option.format.as_str() {
                    "text" | "markdown" => {
                        let (text, cut) = text::bytes(&option.content, text::OPTION_BYTES);
                        OptionBody::Text { text, cut }
                    }
                    // The content is dropped with `option` here.
                    _ => OptionBody::NotDrawn {
                        bytes: option.content.len(),
                    },
                };
                Choice {
                    id: text::chars(&option.option_id, text::TITLE),
                    label: text::chars(&option.label, text::TITLE),
                    summary: optional(option.summary),
                    body,
                }
            })
            .collect();
        Detail {
            id: id.to_owned(),
            title: text::chars(&self.title, text::TITLE),
            workflow: text::chars(&self.workflow, text::TITLE),
            status: self.status.as_deref().map_or(Status::Other, Status::from),
            created: self
                .created_at
                .as_deref()
                .and_then(parse_time)
                .or_else(|| name_time(id)),
            brief: field(&self.brief),
            clarifications: self
                .clarifications
                .items
                .into_iter()
                .map(|item| (field(&item.question), field(&item.answer)))
                .collect(),
            clarifications_omitted: self.clarifications.omitted,
            options,
            options_omitted: self.options.omitted,
            result: self.result.map(|result| Pick {
                pick: result.pick.map(|pick| text::chars(&pick, text::TITLE)),
                comments: result
                    .comments
                    .items
                    .into_iter()
                    .map(|(option, comment)| (text::chars(&option, text::TITLE), field(&comment)))
                    .collect(),
                comments_omitted: result.comments.omitted,
                note: optional(result.note),
            }),
            outcome: self.outcome.map(|outcome| field(&outcome.summary)),
            ratings_total: self.ratings.total,
            ratings: self
                .ratings
                .items
                .into_iter()
                .map(|rating| Rating {
                    value: rating.value,
                    note: optional(rating.note),
                })
                .collect(),
        }
    }
}
