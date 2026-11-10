//! What a page puts in the app's log: its console messages, its uncaught
//! script errors, and the records of what the host refused it. A page's
//! text is untrusted and has no end, so each tab has a budget of records
//! that refills over time. A record without a place is counted, never kept.
use std::{
    borrow::Cow,
    time::{Duration, Instant},
};

/// Bytes of one message that reach the log.
pub(super) const LINE_BYTES: usize = 1024;

/// Records a tab may log at once.
pub(super) const BURST: u32 = 200;

/// Error lines a tab may log at once beyond [`BURST`]. No other record
/// uses them, so a page that fills its console cannot hide the uncaught
/// error that follows.
pub(super) const ERRORS: u32 = 50;

/// The time that gives a tab one record of each count back.
pub(super) const REFILL: Duration = Duration::from_secs(1);

/// A count of records that grows by one each [`REFILL`], up to its size.
#[derive(Debug)]
struct Bucket {
    size: u32,
    left: u32,
    /// When `left` last grew, or was last seen full.
    since: Option<Instant>,
}

impl Bucket {
    const fn new(size: u32) -> Self {
        Self {
            size,
            left: size,
            since: None,
        }
    }

    fn take(&mut self, now: Instant) -> bool {
        self.refill(now);
        let Some(left) = self.left.checked_sub(1) else {
            return false;
        };
        self.left = left;
        true
    }

    /// A clock that steps back gives nothing and takes nothing.
    fn refill(&mut self, now: Instant) {
        let since = *self.since.get_or_insert(now);
        let missing = self.size - self.left;
        let elapsed = now.saturating_duration_since(since);
        let gained = u32::try_from(elapsed.as_nanos() / REFILL.as_nanos()).unwrap_or(u32::MAX);
        if gained >= missing {
            self.left = self.size;
            self.since = Some(now.max(since));
        } else {
            self.left += gained;
            self.since = Some(since + REFILL * gained);
        }
    }

    fn fill(&mut self) {
        self.left = self.size;
        self.since = None;
    }
}

/// What the log is told with a record that has a place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Place {
    /// The tab's records that had no place since the last one that had.
    pub(super) skipped: u32,
    /// Whether the record used the last place and no count said so yet:
    /// the log then gets one line that says that later records are counted.
    pub(super) spent: bool,
}

/// The records one tab's page may still put in the log.
#[derive(Debug)]
pub(super) struct Budget {
    records: Bucket,
    errors: Bucket,
    skipped: u32,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            records: Bucket::new(BURST),
            errors: Bucket::new(ERRORS),
            skipped: 0,
        }
    }
}

impl Budget {
    /// A place for one record at `now`, or nothing when the tab has used
    /// its records. An `error` line uses the places kept for errors first.
    pub(super) fn admit(&mut self, error: bool, now: Instant) -> Option<Place> {
        if !((error && self.errors.take(now)) || self.records.take(now)) {
            self.skipped = self.skipped.saturating_add(1);
            return None;
        }
        let skipped = std::mem::take(&mut self.skipped);
        let empty = self.records.left == 0 && (!error || self.errors.left == 0);
        Some(Place {
            skipped,
            spent: empty && skipped == 0,
        })
    }

    /// All places again, for a load the user asked for. The count of
    /// skipped records stays for the next record to report.
    pub(super) fn renew(&mut self) {
        self.records.fill();
        self.errors.fill();
    }
}

/// The start of `message` that reaches the log, cut on a character, and
/// whether the message was longer.
pub(super) fn bounded(message: &str) -> (&str, bool) {
    let text = &message[..message.floor_char_boundary(LINE_BYTES)];
    (text, text.len() < message.len())
}

/// `text` without the user information, the query, and the fragment of
/// each `scheme://` address in it, and without the query and fragment of
/// each path that starts with `/`. An uncaught error names its script's
/// address, which for an inline script is the document's own: a sign-in's
/// return address would put its code in the log. A page that logs its own
/// requests names them by path, with the same codes. The `:line:column`
/// that ends an address or a path stays.
///
/// An address ends at white space. What follows a `?` or `#` up to there is
/// removed even when it is not part of the address, such as a closing
/// quote: the text may lose a character, never keep a secret. Addresses
/// without `//`, such as `data:`, are the page's own text like the rest of
/// the message and stay.
pub(super) fn without_queries(text: &str) -> Cow<'_, str> {
    match without_address_queries(text) {
        Cow::Borrowed(text) => without_path_queries(text),
        Cow::Owned(text) => Cow::Owned(without_path_queries(&text).into_owned()),
    }
}

fn without_address_queries(text: &str) -> Cow<'_, str> {
    const MARK: &str = "://";
    if !text.contains(MARK) {
        return Cow::Borrowed(text);
    }
    let mut kept = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(MARK) {
        let (before, address) = rest.split_at(at + MARK.len());
        kept.push_str(before);
        let end = address.find(char::is_whitespace).unwrap_or(address.len());
        let (address, after) = address.split_at(end);
        let position = position(address);
        let address = &address[..address.len() - position.len()];
        let path = address.find(['/', '?', '#']).unwrap_or(address.len());
        let query = address.find(['?', '#']).unwrap_or(address.len());
        let host = address[..path].rfind('@').map_or(0, |at| at + 1);
        kept.push_str(&address[host..query]);
        kept.push_str(position);
        rest = after;
    }
    kept.push_str(rest);
    Cow::Owned(kept)
}

/// `text` without the query and fragment of each path in it that starts
/// with `/` and holds a `?`, such as the `/api/users?token=1` of a logged
/// request. A path starts at a `/` that no letter or digit stands before,
/// so the `?` of "either/or?" and of "what? yes" stays, and ends at white
/// space.
fn without_path_queries(text: &str) -> Cow<'_, str> {
    if !(text.contains('/') && text.contains('?')) {
        return Cow::Borrowed(text);
    }
    let mut kept = String::with_capacity(text.len());
    for piece in text.split_inclusive(char::is_whitespace) {
        let word = piece.trim_end_matches(char::is_whitespace);
        let Some((path, query)) = path_query(word) else {
            kept.push_str(piece);
            continue;
        };
        let position = position(&word[path + query..]);
        kept.push_str(&word[..path + query]);
        kept.push_str(position);
        kept.push_str(&piece[word.len()..]);
    }
    Cow::Owned(kept)
}

/// Where the first path with a query starts in `word`, and where its
/// query starts in that path.
fn path_query(word: &str) -> Option<(usize, usize)> {
    let mut before = None;
    word.char_indices().find_map(|(at, c)| {
        let starts = c == '/' && !before.is_some_and(char::is_alphanumeric);
        before = Some(c);
        let path = starts.then(|| &word[at..])?;
        path.contains('?')
            .then(|| path.find(['?', '#']).map(|query| (at, query)))?
    })
}

/// The `:line:column` or `:line` that ends `address`, or nothing.
fn position(address: &str) -> &str {
    let mut start = address.len();
    for _ in 0..2 {
        let head = &address[..start];
        let number = head.trim_end_matches(|c: char| c.is_ascii_digit());
        if number.len() == head.len() || !number.ends_with(':') {
            break;
        }
        start = number.len() - 1;
    }
    &address[start..]
}

#[cfg(test)]
mod tests;
