//! The read of a host: one Python program (`reader.py`), sent on standard
//! input to `python3 -I -` through the host's own shell (and SSH connection
//! on an SSH host), so nothing is written on the host. This computer runs
//! it only for the address of a page; its list of sessions is read in
//! Rust. A read has a deadline that output does not reset. A host without `python3` fails its read with
//! [`Error::NoPython`]; nothing is installed.
//!
//! `list` gives the saved sessions, and whether the host's Walkr server
//! answers `/health` with the pid and protocol of its record (no token is
//! sent); `page ID` gives the address of the private Compare page of that
//! server for session `ID`, and `page` alone the same page without a
//! session. The address holds the
//! secret of that server run: [`Remote`] keeps it in memory only, and its
//! `Debug` shows the port.
use super::Error;
use crate::walkr::{
    history::{
        self, ENTRY_LIMIT,
        pending::{self, Tally},
    },
    run::{self, Parsed, is_session_id},
};
use serde::Deserialize;
use sheprd_client::ScriptLimits;
use sheprd_control_protocol::protocol::walkr::RunAddress;
use std::{
    collections::HashMap,
    fmt,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

use crate::teleport::Host;

/// The reader program.
const PROGRAM: &str = include_str!("reader.py");
/// The exit status of the host's shell when it finds no `python3`.
const NO_PYTHON: i32 = 69;
/// The exit status of the reader at its own deadline (`TIMED_OUT`).
const READER_LATE: i32 = 4;
/// The whole time of one read, output or not.
pub(crate) const DEADLINE: Duration = Duration::from_secs(20);
/// How often the deadline is checked.
const WATCH: Duration = Duration::from_millis(25);
/// The most a host may print for a list.
const LIST_OUTPUT: u64 = 16 * 1024 * 1024;
/// The most a host may print for a page address.
const PAGE_OUTPUT: u64 = 4 * 1024;
/// The longest line of a list: a title of 200 characters, each escaped.
const LINE: usize = 8 * 1024;
/// The path of the private Compare page on a Walkr server.
const PAGE_PATH: &str = "/_walkr/compare/g/";

/// One line of a list.
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Line {
    Session(Record),
    Server(Server),
    End(End),
}

/// A Walkr server of the host's record answers `/health`.
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Server {
    Live,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum End {
    /// No sessions folder: the host counts 0.
    Absent,
    /// The folder held more than `ENTRY_LIMIT` entries; the rest were not
    /// read.
    Cut,
    Read,
}

/// The fields the reader sends of one record.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    created_at: Option<String>,
    repo: Option<String>,
    session_id: String,
    status: Option<String>,
    title: Option<String>,
}

/// The answer of `page`.
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Answer {
    None(Missing),
    Page(String),
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Missing {
    Record,
    Server,
    Session,
    Stopped,
}

/// The page of a session on a host's Walkr server, at its loopback port
/// there.
pub(crate) struct Remote {
    port: u16,
    /// The path and the fragment, which hold the secret.
    rest: String,
}

impl fmt::Debug for Remote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Remote")
            .field("port", &self.port)
            .finish_non_exhaustive()
    }
}

impl Remote {
    /// The port of the Walkr server on the host.
    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    /// The same page through a tunnel at `local` on this computer. The
    /// Walkr server accepts only a loopback `Host` with its own port, so a
    /// tunnel on any other local port gives [`Error::PortBusy`].
    pub(crate) fn tunneled(&self, local: u16) -> Result<Parsed, Error> {
        if local != self.port {
            return Err(Error::PortBusy(self.port));
        }
        self.direct()
    }

    /// The page at the server's own port on `127.0.0.1`: of a server on
    /// this computer, or through a tunnel on that port.
    pub(crate) fn direct(&self) -> Result<Parsed, Error> {
        let address = RunAddress::new(format!("http://127.0.0.1:{}{}", self.port, self.rest));
        run::parse(&address).map_err(|_| Error::PageRefused)
    }
}

/// Runs the reader in `mode` on `host`, stopped at `deadline` or when
/// `cancelled` is set.
fn run(
    host: &Host,
    mode: &str,
    output: u64,
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, Error> {
    let script = format!(
        // i18n: data
        "command -v python3 >/dev/null 2>&1 || exit {NO_PYTHON}\nexec python3 -I - {mode}\n"
    );
    let until = Instant::now() + deadline;
    let stop = AtomicBool::new(false);
    let done = AtomicBool::new(false);
    let mut printed = Vec::new();
    let limits = ScriptLimits {
        output,
        idle: deadline,
    };
    let result = thread::scope(|scope| {
        // The runner's idle limit restarts on each byte; this one does not.
        scope.spawn(|| {
            while !done.load(Ordering::Acquire) {
                if cancelled.load(Ordering::Acquire) || Instant::now() >= until {
                    stop.store(true, Ordering::Release);
                    return;
                }
                thread::sleep(WATCH);
            }
        });
        let result = host.stream(&script, PROGRAM.as_bytes(), &mut printed, limits, &stop);
        done.store(true, Ordering::Release);
        result
    });
    match result {
        Ok(_) => Ok(printed),
        Err(sheprd_client::Error::ScriptCancelled | sheprd_client::Error::ScriptTimeout)
            if !cancelled.load(Ordering::Acquire) =>
        {
            Err(Error::TimedOut)
        }
        Err(sheprd_client::Error::ScriptExit { status, .. })
            if status.code() == Some(NO_PYTHON) =>
        {
            Err(Error::NoPython)
        }
        Err(sheprd_client::Error::ScriptExit { status, .. })
            if status.code() == Some(READER_LATE) =>
        {
            Err(Error::TimedOut)
        }
        Err(error) => Err(error.into()),
    }
}

/// The waiting sessions of `host`.
pub(crate) fn list(
    host: &Host,
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<Tally, Error> {
    parse_list(&run(host, "list", LIST_OUTPUT, deadline, cancelled)?)
}

/// The tally of a list. A line the reader does not write fails the host,
/// and so does a list without its end, which a cut answer lacks.
fn parse_list(output: &[u8]) -> Result<Tally, Error> {
    let text = std::str::from_utf8(output).map_err(|_| Error::Malformed)?;
    let mut rows = Vec::new();
    let mut repos = Vec::new();
    let mut end = None;
    let mut live = false;
    for line in text.lines() {
        if end.is_some() || line.len() > LINE {
            return Err(Error::Malformed);
        }
        match serde_json::from_str::<Line>(line).map_err(|_| Error::Malformed)? {
            Line::Session(record) => {
                // A full list of `ENTRY_LIMIT` rows still takes its end line.
                if rows.len() >= ENTRY_LIMIT || !is_session_id(&record.session_id) {
                    return Err(Error::Malformed);
                }
                rows.push(history::read_row(
                    &record.session_id,
                    record.created_at.as_deref(),
                    record.status.as_deref(),
                    record.title.as_deref(),
                    None,
                ));
                repos.push((record.session_id, record.repo));
            }
            // The reader writes one at most.
            Line::Server(Server::Live) if live => return Err(Error::Malformed),
            Line::Server(Server::Live) => live = true,
            Line::End(kind) => end = Some(kind),
        }
    }
    match end.ok_or(Error::Malformed)? {
        End::Absent => Ok(Tally {
            live,
            complete: true,
            ..Tally::default()
        }),
        kind @ (End::Cut | End::Read) => {
            let complete = matches!(kind, End::Read);
            let mut tally = pending::tally(rows);
            // Each kept page names its repository, as on this computer.
            let mut repos: HashMap<String, String> = repos
                .into_iter()
                .filter_map(|(id, repo)| Some((id, repo?)))
                .filter(|(_, repo)| pending::valid_repo(repo))
                .collect();
            for pending in &mut tally.newest {
                pending.repo = repos.remove(&pending.id);
            }
            tally.live = live;
            tally.complete = complete;
            Ok(tally)
        }
    }
}

/// The page of session `id` on the Walkr server of `host`, or with no `id`
/// the server's Compare page itself. Only a read: the reader asks the
/// server for its capabilities and its page path.
pub(crate) fn page(
    host: &Host,
    id: Option<&str>,
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<Remote, Error> {
    let mode = match id {
        Some(id) if !is_session_id(id) => return Err(Error::PageRefused),
        Some(id) => format!("page {id}"),
        None => "page".to_owned(),
    };
    let output = run(host, &mode, PAGE_OUTPUT, deadline, cancelled)?;
    parse_page(&output, id)
}

/// Whether `path` is a whole Compare page path: `PAGE_PATH` and 1 to 256
/// lowercase hex digits, so no query, no other segment and no dot segment.
fn is_page_path(path: &str) -> bool {
    path.strip_prefix(PAGE_PATH).is_some_and(|id| {
        (1..=256).contains(&id.len())
            && id
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    })
}

/// The address in the answer of `page`: `http` on a loopback host with a
/// port and no user info, the page path, and `id` as its fragment, or no
/// fragment at all without an `id`.
pub(crate) fn parse_page(output: &[u8], id: Option<&str>) -> Result<Remote, Error> {
    let text = std::str::from_utf8(output).map_err(|_| Error::Malformed)?;
    let address = match serde_json::from_str::<Answer>(text.trim()).map_err(|_| Error::Malformed)? {
        Answer::None(Missing::Record) => return Err(Error::NoServer),
        Answer::None(Missing::Server) => return Err(Error::OtherServer),
        Answer::None(Missing::Session) => return Err(Error::PageRefused),
        Answer::None(Missing::Stopped) => return Err(Error::ServerStopped),
        Answer::Page(address) => address,
    };
    let parsed = run::parse(&RunAddress::new(address.as_str())).map_err(|_| Error::PageRefused)?;
    let fragment = address.split_once('#').map(|(_, fragment)| fragment);
    if parsed.session() != id || (id.is_none() && fragment.is_some()) {
        return Err(Error::PageRefused);
    }
    // `parse` proved the shape `http://HOST:PORT` before the path.
    let (authority, rest) = address
        .strip_prefix("http://")
        .and_then(|rest| rest.find('/').map(|at| rest.split_at(at)))
        .filter(|(_, rest)| is_page_path(rest.split_once('#').map_or(rest, |(path, _)| path)))
        .ok_or(Error::PageRefused)?;
    let port = authority
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse().ok())
        .filter(|port| *port != 0)
        .ok_or(Error::PageRefused)?;
    Ok(Remote {
        port,
        rest: rest.to_owned(),
    })
}
