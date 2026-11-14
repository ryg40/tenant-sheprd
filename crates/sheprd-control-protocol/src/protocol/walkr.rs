//! Walkr connectivity as observed by this computer, without browser credentials.
use serde::{Deserialize, Serialize};
use std::fmt;

/// The longest a status request waits for a fresh probe.
pub const WAIT_SECONDS: u64 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum Connectivity {
    NotConfigured,
    Checking,
    Unreachable {
        cause: Unreachable,
    },
    NetworkRefused,
    Down {
        status: u16,
    },
    Unexpected {
        status: u16,
    },
    Reachable,
    /// The section's page last loaded on the Walkr origin.
    Ready,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unreachable {
    Dns,
    Connect,
    Timeout,
    Tls,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum WalkrOp {
    // An empty struct variant makes serde reject extra operation fields.
    Status {},
    /// The caller's pane started a Walkr server at `address`. No operation
    /// closes the section's page: the user may still rate on it.
    Connect {
        address: RunAddress,
        /// Also show the section, as `ui panel walkr` does.
        show: bool,
    },
    /// The caller's pane no longer holds the run. The page stays.
    Disconnect {},
}

/// The page address of a Walkr run. Its path holds the secret of the server
/// run, so `Debug` prints host and port only and there is no `Display`. The
/// app validates it; the caller parses nothing.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunAddress(String);

impl RunAddress {
    /// The longest address the app accepts.
    pub const MAX_BYTES: usize = 2048;

    pub fn new(address: impl Into<String>) -> Self {
        Self(address.into())
    }

    /// The whole address, secret included. Only the validation of the app
    /// reads it; it must reach no log, reply, or file.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Host and port, when the text has the shape of an address.
    fn authority(&self) -> Option<&str> {
        let (_, rest) = self.0.split_once("://")?;
        let authority = rest.split(['/', '\\', '?', '#']).next()?;
        let host = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        // A host name is at most 253 bytes; a port adds six.
        let plain = !host.is_empty()
            && host.len() <= 259
            && host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-:[]".contains(&byte));
        plain.then_some(host)
    }
}

impl fmt::Debug for RunAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RunAddress")
            .field(&self.authority().unwrap_or("<redacted>"))
            .finish()
    }
}

/// Where the section's address comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    #[default]
    None,
    /// A server that `walkr connect` named.
    Run,
    /// The `walkr.url` setting.
    Override,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub connectivity: Connectivity,
    /// Host and port only; never a path, query, or page text.
    pub address: String,
    pub checked_seconds_ago: Option<u64>,
    pub embedded: bool,
    pub source: Source,
    /// Whether a pane holds the run. A run whose page is kept after the last
    /// `disconnect` has the source `run` and is not live.
    pub live: bool,
    /// The saved Compare sessions of each host, counted only: no title or
    /// text of a session.
    pub sessions: Vec<HostSessions>,
}

/// How many saved Compare sessions the section lists for a host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostSessions {
    /// `local` for this computer, else the SSH target of the host.
    pub host: String,
    /// The listed rows: at most the newest 100.
    pub count: u32,
    /// The sessions whose status is `open`: they wait for a pick.
    pub waiting: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reply {
    Status(Status),
    Connected {
        /// Host and port only.
        address: String,
        /// Whether a run of another server was replaced.
        replaced: bool,
    },
}

#[cfg(test)]
mod tests;
