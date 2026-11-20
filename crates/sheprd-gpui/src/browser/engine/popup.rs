//! New windows a page asks for: a link with `target=_blank`, a Ctrl+click
//! or a middle click on a link, and `window.open`.
//!
//! Servo asks its host for each one (`WebViewDelegate::request_create_new`)
//! and waits for the answer. At the pin it asks after any script, with or
//! without the user's input: its popup blocker is a TODO (step 8 of
//! `choose_a_navigable` in Servo's `windowproxy.rs`), and it only consumes
//! the page's user activation. So the host admits one new window for each
//! press of the user in the page: a button that goes down over it, or a key
//! that goes down while it has the keyboard ([`activation`]). A release, a
//! move, and a wheel turn give nothing and renew nothing. The window stamps
//! the press as it sends it, and the grant ends [`GRANT`] after that, or
//! earlier with every other grant of the page ([`super::clipboard`]): as the
//! page starts a load, a reload, or a history step, shows another address,
//! hides, or loses the keyboard, and as its window stops being the active
//! one. So a document that the page leaves for gets no grant of a press in
//! the one before. Servo names the page of a request, not the frame or the
//! script that asked: within [`GRANT`], any script of the document that got
//! the press, a timer too, and any frame of the page can use it. A request
//! without a grant is refused, as is every request of the Walkr section's
//! pages, which have no tab to open beside.
//!
//! The new page is the web view that Servo builds from the request, so its
//! address, its referrer, `rel=noopener`, and `window.opener` are Servo's.
//! The request names no address: Servo builds the page at `about:blank` and
//! then navigates it. The engine holds that first navigation and offers the
//! page to the opener's window ([`Opening`]), which answers
//! [`Command::Adopted`] once a tab holds it; only then does the navigation
//! go on. A first navigation to another scheme or after [`WAIT`], and a page
//! that no tab holds by then, close the page: a refused adoption and a lost
//! answer end that way, and no new page loads its address without a tab. So
//! `window.open()` with no address opens no tab. The engine checks the same
//! deadline as the window's word arrives: a page whose time is up ends, and
//! its window takes it in no tab, or closes the tab it took it in. A window
//! that cannot send the word closes the tab at once. No tab loads the
//! page's address as a page of its own, which would lose the navigation
//! that the page made: its request, its opener, and a form's data. The
//! request does not say whether a Ctrl+click or a middle click asked for
//! it, so the new tab always shows. The window features of `window.open`
//! (size, place, `popup`) are not applied: the page is a tab like any other.
use super::{
    Command, Engine, Latest, Mailbox, Open, PageKey,
    clipboard::{Grant, Waiting},
};
use crate::browser::{
    WebUrl,
    reports::{Event, Source},
};
use servo::{InputEvent, Key, KeyState, MouseButtonAction, NamedKey, NavigationRequest};
use std::{
    cell::{Cell, RefCell},
    sync::Arc,
    time::{Duration, Instant},
};

mod error;

pub(super) use error::Error;

/// How long one press of the user lets its page open a new window, from
/// the press in the window. Browsers keep the user's activation for 5 s
/// (Servo's own default too); this is shorter.
const GRANT: Duration = Duration::from_secs(2);

/// How long a page that Servo built for a new window has, from then, to
/// make its first navigation and to be held by a tab.
const WAIT: Duration = Duration::from_secs(10);

/// The new window that `event`, sent `now` to a page whose grants `waiting`
/// keeps, lets the page open: a button that goes down, or a key that goes
/// down. As in browsers, a key that repeats gives nothing, and neither does
/// Escape.
pub(in crate::browser) fn activation(
    event: &InputEvent,
    waiting: &Waiting,
    now: Instant,
) -> Option<Grant> {
    let pressed = match event {
        InputEvent::MouseButton(button) => button.action == MouseButtonAction::Down,
        InputEvent::Keyboard(key) => {
            let key = &key.event;
            key.state == KeyState::Down && !key.repeat && key.key != Key::Named(NamedKey::Escape)
        }
        _ => false,
    };
    pressed.then(|| waiting.grant(now, GRANT))
}

/// Whether a page may open a new window now: once for each press.
#[derive(Default)]
pub(super) struct Activation(Cell<Option<Grant>>);

impl Activation {
    /// Keeps the grant the window put on an input of the page, if it put
    /// one. An input without one changes nothing.
    pub(super) fn pressed(&self, grant: Option<Grant>) {
        if grant.is_some() {
            self.0.set(grant);
        }
    }

    /// Uses the grant, if one still holds `now` in the page's `waiting`.
    pub(super) fn admit(&self, waiting: &Waiting, now: Instant) -> bool {
        self.0.take().is_some_and(|grant| waiting.holds(grant, now))
    }
}

/// Where a page that Servo built for a new window stands.
#[derive(Clone, Copy)]
enum Stage {
    // keep-sorted start
    /// A tab of its window holds it.
    Given,
    /// Its window was offered it at its first navigation, which waits for
    /// the window's word that a tab holds the page, until `deadline`.
    Offered { deadline: Instant },
    /// It goes to no tab: the host closes it.
    Refused,
    /// It has not navigated yet: `opener` asked for it, and it ends at
    /// `deadline`.
    Waiting { deadline: Instant, opener: Source },
    // keep-sorted end
}

/// A page's place as a new window. A page that a window opened has none.
#[derive(Default)]
pub(super) struct Opening {
    stage: Cell<Option<Stage>>,
    /// The first navigation, while it waits for the page's tab.
    held: RefCell<Option<NavigationRequest>>,
}

impl Opening {
    /// For a page that Servo built at `now` for `opener`'s new window.
    pub(super) fn new(opener: Source, now: Instant) -> Self {
        // At the end of the clock, a page that ends at once.
        let deadline = now.checked_add(WAIT).unwrap_or(now);
        let stage = Stage::Waiting { deadline, opener };
        Self {
            stage: Cell::new(Some(stage)),
            held: RefCell::default(),
        }
    }

    /// Whether the page of `open` may navigate to `url` at `now`, which the
    /// page rules `allow` or not; `None` while the navigation waits for a
    /// tab ([`Self::hold`]). The first navigation of a page built for a new
    /// window offers the page to its opener's window when it goes in time to
    /// a web address that the rules allow, and refuses the page otherwise.
    pub(super) fn navigates(
        &self,
        url: &url::Url,
        allow: bool,
        open: &Open,
        now: Instant,
    ) -> Option<bool> {
        let (deadline, opener) = match self.stage.get() {
            None | Some(Stage::Given) => return Some(allow),
            // The page makes no other navigation until its tab holds it.
            Some(Stage::Offered { .. } | Stage::Refused) => return Some(false),
            Some(Stage::Waiting { deadline, opener }) => (deadline, opener),
        };
        if self.ended(open, now) {
            return Some(false);
        }
        let web = WebUrl::try_from(url.as_str()).ok().filter(|_| allow);
        let offered = web.is_some_and(|url| {
            // Dropped with the event, the popup closes the page.
            let opened = Event::Opened(opener, url, open.source, Popup::new(open));
            open.reports.try_send(opened).is_ok()
        });
        if !offered {
            self.refuse(open);
            return Some(false);
        }
        self.stage.set(Some(Stage::Offered { deadline }));
        None
    }

    /// Keeps `request`, the navigation that waits for the page's tab.
    pub(super) fn hold(&self, request: NavigationRequest) {
        if let Some(replaced) = self.held.replace(Some(request)) {
            replaced.deny();
        }
    }

    /// The engine's answer at `now` to the window's word that a tab holds
    /// the page: whether the page goes on in it, its first navigation with
    /// it. A page whose time is up by then ends, as at its deadline, and
    /// its window closes the tab.
    pub(super) fn adopted(&self, open: &Open, now: Instant) -> bool {
        if self.ended(open, now) {
            return false;
        }
        if let Some(Stage::Offered { .. }) = self.stage.get() {
            self.stage.set(Some(Stage::Given));
            if let Some(request) = self.held.take() {
                request.allow();
            }
        }
        matches!(self.stage.get(), Some(Stage::Given))
    }

    /// When the host closes the page unless its window adopts it first.
    pub(super) fn deadline(&self) -> Option<Instant> {
        match self.stage.get() {
            Some(Stage::Offered { deadline } | Stage::Waiting { deadline, .. }) => Some(deadline),
            Some(Stage::Given | Stage::Refused) | None => None,
        }
    }

    /// Whether the host closes the page at `now`: it goes to no tab, or
    /// its time is up, which ends it here. The one check of the page's
    /// deadline, for its first navigation, its adoption, and its end alike,
    /// so no tab adopts a page that its deadline closed or closes.
    pub(super) fn ended(&self, open: &Open, now: Instant) -> bool {
        let deadline = match self.stage.get() {
            Some(Stage::Refused) => return true,
            Some(Stage::Offered { deadline } | Stage::Waiting { deadline, .. }) => deadline,
            Some(Stage::Given) | None => return false,
        };
        let ended = now > deadline;
        if ended {
            self.refuse(open);
        }
        ended
    }

    /// Sends the page to no tab and denies its held navigation. A window
    /// that was offered it takes it in no tab, or closes the one it took
    /// it in ([`Mailbox::refused`]).
    fn refuse(&self, open: &Open) {
        let offered = self.stage.replace(Some(Stage::Refused));
        if let Some(request) = self.held.take() {
            request.deny();
        }
        if matches!(offered, Some(Stage::Offered { .. })) {
            open.mailbox.refused.set();
            open.notifier.notify();
        }
    }
}

impl Drop for Opening {
    fn drop(&mut self) {
        // Servo allows a navigation that nobody answers.
        if let Some(request) = self.held.get_mut().take() {
            request.deny();
        }
    }
}

/// A page that Servo built for a new window, on its way to the window that
/// adopts it as a tab. Dropped before that, it closes the page; if the
/// engine's queue is full then, the engine closes it at its deadline.
pub(crate) struct Popup {
    key: PageKey,
    mailbox: Arc<Mailbox>,
    latest: Arc<Latest>,
    /// Closes the page when the popup is dropped; `None` once adopted.
    engine: Option<Engine>,
}

impl Popup {
    fn new(open: &Open) -> Self {
        Self {
            key: open.key,
            mailbox: open.mailbox.clone(),
            latest: open.latest.clone(),
            engine: Engine::started(),
        }
    }

    /// Whether the engine ended the page before its window took it: no tab
    /// may hold it then. Asked once, as the window takes the page.
    pub(in crate::browser) fn ended(&self) -> bool {
        self.mailbox.refused.take()
    }

    /// Tells the engine that a tab of the window holds the page now, and
    /// hands over the page's key, its mailbox, and the latest place and
    /// size, for the window's handle to it, which closes it from then on.
    /// The engine answers through the mailbox: a page whose time is up
    /// ends there, and its tab closes ([`Mailbox::refused`]). An engine
    /// that did not take the word ([`super::Error::Busy`] or
    /// [`super::Error::Stopped`]) closes the page at its deadline, and the
    /// window closes the tab.
    pub(in crate::browser) fn adopt(
        mut self,
    ) -> Result<(PageKey, Arc<Mailbox>, Arc<Latest>), super::Error> {
        let engine = self.engine.take().ok_or(super::Error::Stopped)?;
        engine.send(Command::Adopted(self.key))?;
        Ok((self.key, self.mailbox.clone(), self.latest.clone()))
    }
}

impl Drop for Popup {
    fn drop(&mut self) {
        if let Some(engine) = self.engine.take() {
            // A full queue leaves the page to its deadline.
            let _ = engine.send(Command::Close(self.key));
        }
    }
}

#[cfg(test)]
mod tests;
