//! One Walkr server that panes of this window connected, and its address.
//!
//! The address holds the secret of the server run in its path. It lives in
//! memory only, in [`Address`], and leaves it only for the section's page.
use super::Error;
use crate::browser::WebUrl;
use sheprd_control_protocol::protocol::walkr::RunAddress;
use std::fmt;

/// The most panes one run remembers. One more is refused: no pane loses the
/// run to another one.
pub(crate) const MAX_PANES: usize = 64;

/// The address of the section's page: a run's, which holds its secret, or
/// the setting's. `Debug` prints host and port only.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Address(WebUrl);

#[cfg(embedded_pages)]
impl Address {
    /// The whole address, for the engine that loads the section's page.
    /// Only the pages' `ensure_walkr` asks.
    pub(crate) fn expose(&self) -> &str {
        self.0.as_str()
    }
}

/// The `walkr.url` setting as the address of the section's page.
#[cfg(embedded_pages)]
impl From<WebUrl> for Address {
    fn from(url: WebUrl) -> Self {
        Self(url)
    }
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Address").field(&self.0.address()).finish()
    }
}

/// A pane that holds a run: its ID and the daemon it belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pane {
    pub(crate) id: String,
    pub(crate) daemon: Option<String>,
    /// The daemon boot whose snapshot last listed the pane.
    seen: Option<String>,
}

impl Pane {
    pub(crate) fn new(id: impl Into<String>, daemon: Option<String>) -> Self {
        Self {
            id: id.into(),
            daemon,
            seen: None,
        }
    }

    fn is(&self, other: &Self) -> bool {
        self.id == other.id && self.daemon == other.daemon
    }
}

/// A validated address, before it becomes or joins a run.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Parsed {
    address: Address,
    origin: WebUrl,
    session: Option<String>,
}

// By hand, as for `Run`: the session ID stays out of logs.
impl fmt::Debug for Parsed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Parsed")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl Parsed {
    /// Host and port, for a reply.
    pub(crate) fn host(&self) -> String {
        self.origin.address()
    }
}

/// `http` on a loopback host with a port, no user info, within the bound.
/// No error names any part of the address.
pub(crate) fn parse(address: &RunAddress) -> Result<Parsed, Error> {
    let text = address.expose();
    if text.len() > RunAddress::MAX_BYTES {
        return Err(Error::AddressTooLong);
    }
    let authority = text
        .strip_prefix("http://")
        .and_then(|rest| rest.split(['/', '\\', '?', '#']).next())
        .ok_or(Error::InvalidAddress)?;
    // The written authority: URL parsing hides empty user info and a
    // default port.
    let port = ["127.0.0.1:", "localhost:", "[::1]:"]
        .iter()
        .find_map(|host| authority.strip_prefix(host))
        .ok_or(Error::InvalidAddress)?;
    if port.is_empty() || port.len() > 5 || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::InvalidAddress);
    }
    let url = WebUrl::try_from(text).map_err(|_| Error::InvalidAddress)?;
    let origin = WebUrl::try_from(url.origin()).map_err(|_| Error::InvalidAddress)?;
    if url.loopback_port().is_none() {
        return Err(Error::InvalidAddress);
    }
    let session = text
        .split_once('#')
        .map(|(_, fragment)| fragment)
        .filter(|fragment| is_session_id(fragment))
        .map(str::to_owned);
    Ok(Parsed {
        address: Address(url),
        origin,
        session,
    })
}

/// `YYYYMMDDTHHMMSSZ-` and twelve lowercase hex digits.
pub(super) fn is_session_id(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 29
        && bytes.iter().enumerate().all(|(at, byte)| match at {
            8 => *byte == b'T',
            15 => *byte == b'Z',
            16 => *byte == b'-',
            0..=14 => byte.is_ascii_digit(),
            _ => matches!(byte, b'0'..=b'9' | b'a'..=b'f'),
        })
}

/// What a `connect` did to the window's run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Connected {
    /// A run of another server gave way.
    pub(crate) replaced: bool,
    /// The run of the same server has a newer address.
    pub(crate) moved: bool,
}

/// What a `disconnect` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Disconnected {
    /// The pane held no run.
    NotHeld,
    /// Another pane still holds the run.
    StillLive,
    /// No pane holds the run; its page stays.
    Ended,
    /// No pane held the run and it had no page, so it is forgotten.
    Dropped,
}

pub(crate) struct Run {
    address: Address,
    origin: WebUrl,
    session: Option<String>,
    panes: Vec<Pane>,
    /// Tells this run from the one before it, for the page made for it.
    serial: u64,
}

// By hand: the session ID is the owner's value, and stays out of logs too.
impl fmt::Debug for Run {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Run")
            .field("address", &self.address)
            .field("live", &self.live())
            .field("panes", &self.panes.len())
            .field("serial", &self.serial)
            .finish_non_exhaustive()
    }
}

impl Run {
    pub(super) fn new(parsed: Parsed, pane: Pane, serial: u64) -> Self {
        Self {
            address: parsed.address,
            origin: parsed.origin,
            session: parsed.session,
            panes: vec![pane],
            serial,
        }
    }

    /// Whether the pane may join: it holds the run already, or the run has
    /// room for one more.
    pub(super) fn admits(&self, pane: &Pane) -> bool {
        self.panes.len() < MAX_PANES || self.panes.iter().any(|held| held.is(pane))
    }

    /// The same server: the pane joins and the newer address counts.
    /// Whether the address changed. The caller asked [`Self::admits`].
    pub(super) fn join(&mut self, parsed: Parsed, pane: Pane) -> bool {
        let moved = self.address != parsed.address;
        self.address = parsed.address;
        self.session = parsed.session;
        if !self.panes.iter().any(|held| held.is(&pane)) {
            self.panes.push(pane);
        }
        moved
    }

    /// Removes the pane. Whether it held the run.
    pub(super) fn leave(&mut self, pane: &Pane) -> bool {
        let before = self.panes.len();
        self.panes.retain(|held| !held.is(pane));
        self.panes.len() != before
    }

    /// Drops each pane that its daemon listed before and lists no more.
    /// `listed` gives, for a pane, the boot of its daemon's snapshot and
    /// whether that snapshot holds the pane; `None` when no snapshot of that
    /// daemon is at hand. A restarted daemon proves nothing about a pane.
    pub(super) fn observe<'a>(
        &mut self,
        mut listed: impl FnMut(&Pane) -> Option<(&'a str, bool)>,
    ) -> bool {
        let before = self.panes.len();
        self.panes.retain_mut(|pane| match listed(pane) {
            Some((boot, true)) => {
                if pane.seen.as_deref() != Some(boot) {
                    pane.seen = Some(boot.to_owned());
                }
                true
            }
            Some((boot, false)) => pane.seen.as_deref() != Some(boot),
            None => true,
        });
        self.panes.len() != before
    }

    /// Whether a pane holds the run.
    pub(crate) fn live(&self) -> bool {
        !self.panes.is_empty()
    }

    /// Scheme, host, and port: what the probe asks and a page is compared to.
    pub(crate) fn origin(&self) -> &WebUrl {
        &self.origin
    }

    /// Host and port, for the header and a reply.
    pub(crate) fn host(&self) -> String {
        self.origin.address()
    }

    #[cfg(embedded_pages)]
    pub(crate) fn address(&self) -> &Address {
        &self.address
    }

    pub(crate) fn serial(&self) -> u64 {
        self.serial
    }

    pub(super) fn is_server(&self, parsed: &Parsed) -> bool {
        self.origin == parsed.origin
    }

    /// The Compare session the address names, for the history's live mark.
    pub(crate) fn session(&self) -> Option<&str> {
        self.session.as_deref()
    }
}
