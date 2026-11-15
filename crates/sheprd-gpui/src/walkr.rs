//! On-demand Walkr connectivity. Only the worker performs network I/O.
#[cfg(any(embedded_pages, test))]
pub(crate) mod document;
mod error;
mod header;
pub(crate) mod history;
mod history_view;
#[cfg(windows)]
pub(crate) mod privacy;
mod probe;
mod resolver;
pub(crate) mod run;
mod section;
pub(crate) mod sessions;
pub(crate) mod waiting;
mod waiting_view;
mod window;
mod worker;

#[cfg(test)]
mod tests;

pub(crate) use error::Error;
pub(crate) use run::Disconnected;
pub(crate) use sheprd_control_protocol::protocol::walkr::{Connectivity, Unreachable};

use crate::config::WalkrConfig;
use probe::Page;
use run::{Connected, Run};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
use worker::{Command, Update, Worker};

pub(crate) const INTERVAL: Duration = Duration::from_secs(30);
pub(crate) const DEMAND: Duration = Duration::from_secs(60);
/// A page with no activity for this long closes by itself.
#[cfg(embedded_pages)]
pub(crate) const IDLE_CLOSE: Duration = Duration::from_secs(2 * 60 * 60);
/// What the probe asks a run for: every Walkr server serves it.
const RUN_PROBE_PATH: &str = "/sdk.js";
/// The longest host of another page that the header shows.
#[cfg(embedded_pages)]
const HOST_LIMIT: usize = 253;

/// What the section's page was made for, without the address of a run.
#[cfg(embedded_pages)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PageOf {
    /// The `walkr.url` setting.
    Override(crate::browser::WebUrl),
    /// The page of a waiting session on an SSH host, with this serial.
    Remote(u64),
    /// The run with this serial.
    Run(u64),
}

#[derive(Debug)]
pub(crate) struct Walkr {
    busy: bool,
    config: WalkrConfig,
    demand: Option<Instant>,
    /// Close or the idle limit closed the page: none starts by itself.
    #[cfg(embedded_pages)]
    dismissed: bool,
    generation: u64,
    header: header::Header,
    last: Option<Update>,
    /// When the page last had activity; see `docs/walkr.md`.
    last_activity: Instant,
    page: Page,
    #[cfg(embedded_pages)]
    pub(crate) page_error: Option<gpui::SharedString>,
    /// What the open page, or the one that failed, was made for.
    #[cfg(embedded_pages)]
    pub(crate) page_for: Option<PageOf>,
    #[cfg(embedded_pages)]
    pub(crate) page_requested: bool,
    refresh: bool,
    run: Option<Run>,
    /// Numbers the runs of this window.
    serial: u64,
    /// The page of a saved session; see `browser::walkr_session`.
    #[cfg(embedded_pages)]
    pub(crate) session_page: crate::browser::SessionPage,
    /// The saved sessions the section lists.
    pub(crate) sessions: sessions::Sessions,
    shown: bool,
    start_error: Option<Error>,
    /// The pages that wait for a pick on every counted host.
    pub(crate) waiting: waiting::Waiting,
    worker: Option<Worker>,
}

impl Default for Walkr {
    fn default() -> Self {
        Self {
            busy: false,
            config: WalkrConfig::default(),
            demand: None,
            #[cfg(embedded_pages)]
            dismissed: false,
            generation: 0,
            header: header::Header::default(),
            last: None,
            last_activity: Instant::now(),
            page: Page::default(),
            #[cfg(embedded_pages)]
            page_error: None,
            #[cfg(embedded_pages)]
            page_for: None,
            #[cfg(embedded_pages)]
            page_requested: false,
            refresh: false,
            run: None,
            serial: 0,
            #[cfg(embedded_pages)]
            session_page: crate::browser::SessionPage::default(),
            sessions: sessions::Sessions::default(),
            shown: false,
            start_error: None,
            waiting: waiting::Waiting::default(),
            worker: None,
        }
    }
}

impl Walkr {
    pub(crate) fn poll(&mut self, shown: bool, config: &WalkrConfig, now: Instant) -> bool {
        self.show(shown, now);
        let mut changed = self.poll_worker(shown, config, now);
        let live = self.live_session().map(str::to_owned);
        changed |= self.sessions.poll(shown, live.as_deref(), now);
        let busy = self.busy();
        let busy_changed = self.busy != busy;
        self.busy = busy;
        self.header.update(self.label());
        changed || busy_changed
    }

    /// The section shows, or hides. From hidden to shown is activity.
    fn show(&mut self, shown: bool, now: Instant) {
        if shown && !self.shown {
            self.refresh = true;
            self.want_history();
            self.active(now);
            // The override's page loads again when its section next shows.
            #[cfg(embedded_pages)]
            if self.run.is_none() {
                self.dismissed = false;
            }
        }
        self.shown = shown;
    }

    fn poll_worker(&mut self, shown: bool, config: &WalkrConfig, now: Instant) -> bool {
        let mut changed = self.config != *config;
        if changed {
            self.config = config.clone();
            // A run has priority: the setting is not the probe's target then.
            if self.run.is_none() {
                self.retarget();
            }
        }
        // Retain a stopping handle until termination; demand can return mid-probe.
        if self.worker.as_ref().is_some_and(Worker::is_finished) {
            self.worker = None;
        }
        let wanted = self.configured() && (shown || self.demand.is_some_and(|until| now < until));
        if !wanted {
            if let Some(worker) = &self.worker
                && !worker.stopping()
            {
                worker.stop();
                self.generation = self.generation.wrapping_add(1);
            }
            self.demand = None;
            self.start_error = None;
            self.refresh = false;
            return changed;
        }
        if self.worker.as_ref().is_some_and(Worker::stopping) {
            return changed;
        }
        if self.worker.is_none() && self.start_error.is_none() {
            match worker::start() {
                Ok(worker) => {
                    self.worker = Some(worker);
                    self.refresh = true;
                }
                Err(error) => {
                    self.start_error = Some(error);
                    return true;
                }
            }
        }
        let target = self.target();
        if self.refresh
            && let (Some(worker), Some((url, path))) = (&self.worker, target)
        {
            self.refresh = !worker.send(Command::Refresh {
                generation: self.generation,
                url,
                path,
            });
        }
        while let Some(worker) = &self.worker {
            match worker.updates.try_recv() {
                Ok(update) => changed |= self.apply(update),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
        changed
    }

    fn apply(&mut self, update: Update) -> bool {
        if self.generation != update.generation
            || self
                .last
                .as_ref()
                .is_some_and(|last| last.checked > update.checked)
        {
            return false;
        }
        self.last = Some(update);
        true
    }

    pub(crate) fn demand(&mut self, until: Instant) {
        self.demand = Some(self.demand.map_or(until, |known| known.max(until)));
        self.sessions.demand(until);
    }

    /// The list and the waiting count are due: the section showed, or a
    /// run connected or ended.
    fn want_history(&mut self) {
        self.sessions.want_list();
        self.waiting.want();
    }

    /// The session whose row is live; see [`sessions::live_session`].
    pub(crate) fn live_session(&self) -> Option<&str> {
        sessions::live_session(self.run.as_ref(), self.connectivity())
    }

    /// Whether the content shows a saved session instead of the page: a
    /// row is picked, it is not live, and its page from this computer's
    /// Walkr server is not open.
    pub(crate) fn detail_shown(&self) -> bool {
        self.sessions.pick().is_some_and(|pick| {
            self.live_session() != Some(pick) && !self.waiting.local_page_open(pick)
        })
    }

    pub(crate) fn refresh(&mut self) {
        self.start_error = None;
        self.refresh = true;
    }

    pub(crate) fn busy(&self) -> bool {
        (self.refresh && self.start_error.is_none())
            || self.worker.as_ref().is_some_and(Worker::busy)
    }

    /// A new probe target: results of the one before it no longer count.
    fn retarget(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.last = None;
        self.start_error = None;
        self.refresh = true;
        #[cfg(embedded_pages)]
        {
            self.dismissed = false;
        }
    }

    fn configured(&self) -> bool {
        self.run.is_some() || self.config.url.is_some()
    }

    /// What the probe asks: the run's origin, else the `walkr.url` setting.
    fn target(&self) -> Option<(crate::browser::WebUrl, String)> {
        match &self.run {
            Some(run) => Some((run.origin().clone(), RUN_PROBE_PATH.to_owned())),
            None => Some((self.config.url.clone()?, self.config.probe_path.clone())),
        }
    }

    pub(crate) fn run(&self) -> Option<&Run> {
        self.run.as_ref()
    }

    /// The caller's pane holds a Walkr server at `parsed`. The same server
    /// gains the pane and takes the newer address; another one replaces the
    /// run. The caller closes the page of a replaced run. A run that holds
    /// its most panes refuses one more, and nothing changes.
    pub(crate) fn connect(
        &mut self,
        parsed: run::Parsed,
        pane: run::Pane,
        now: Instant,
    ) -> Result<Connected, Error> {
        if let Some(run) = &self.run
            && run.is_server(&parsed)
            && !run.admits(&pane)
        {
            return Err(Error::TooManyPanes);
        }
        self.active(now);
        self.want_history();
        // The page of the run takes the place of a remote page. A page of
        // this computer, open or opening, also drops its pick, so that the
        // section shows the run's page and not that session's text.
        if self.waiting.remote.as_ref().is_some_and(|remote| {
            remote.machine.is_local() && !matches!(remote.state, waiting::RemoteState::Failed(_))
        }) {
            self.sessions.back();
        }
        self.waiting.close_remote();
        #[cfg(embedded_pages)]
        {
            self.dismissed = false;
            self.page_requested = true;
        }
        let connected = match &mut self.run {
            Some(run) if run.is_server(&parsed) => Connected {
                replaced: false,
                moved: run.join(parsed, pane),
            },
            current => {
                let replaced = current.is_some();
                self.serial = self.serial.wrapping_add(1);
                *current = Some(Run::new(parsed, pane, self.serial));
                self.retarget();
                Connected {
                    replaced,
                    moved: false,
                }
            }
        };
        self.header.update(self.label());
        Ok(connected)
    }

    /// The pane lets go of the run. With no pane left the run is ended: its
    /// page stays, and without a page the run is forgotten at once.
    pub(crate) fn disconnect(&mut self, pane: &run::Pane) -> Disconnected {
        let Some(run) = &mut self.run else {
            return Disconnected::NotHeld;
        };
        if !run.leave(pane) {
            return Disconnected::NotHeld;
        }
        if run.live() {
            return Disconnected::StillLive;
        }
        self.ended()
    }

    /// Forgets each pane its daemon no longer lists, as a `disconnect` of it.
    /// Whether the run changed. See [`Run::observe`] for `listed`.
    pub(crate) fn observe_panes<'a>(
        &mut self,
        listed: impl FnMut(&run::Pane) -> Option<(&'a str, bool)>,
    ) -> bool {
        let Some(run) = self.run.as_mut().filter(|run| run.live()) else {
            return false;
        };
        if !run.observe(listed) {
            return false;
        }
        if !run.live() {
            self.ended();
        }
        true
    }

    fn ended(&mut self) -> Disconnected {
        self.want_history();
        let dropped = self.settle();
        self.header.update(self.label());
        if dropped {
            Disconnected::Dropped
        } else {
            Disconnected::Ended
        }
    }

    /// Forgets an ended run that has no page, with its address. Whether it did.
    fn settle(&mut self) -> bool {
        if self.run.as_ref().is_none_or(Run::live) || self.page_open() {
            return false;
        }
        self.run = None;
        self.want_history();
        self.retarget();
        true
    }

    /// Whether the section has a page that did not fail.
    pub(crate) fn page_open(&self) -> bool {
        #[cfg(embedded_pages)]
        {
            self.page_for.is_some() && self.page_error.is_none()
        }
        #[cfg(not(embedded_pages))]
        {
            false
        }
    }

    /// Activity starts the idle limit of the page again.
    pub(crate) fn active(&mut self, now: Instant) {
        self.last_activity = self.last_activity.max(now);
    }

    /// Whether the page had no activity for [`IDLE_CLOSE`].
    #[cfg(embedded_pages)]
    pub(crate) fn idle(&self, now: Instant) -> bool {
        self.page_open() && now.saturating_duration_since(self.last_activity) >= IDLE_CLOSE
    }

    /// What a page of this window would be made for now.
    #[cfg(embedded_pages)]
    pub(crate) fn source(&self, config: &WalkrConfig) -> Option<PageOf> {
        if let Some((_, serial)) = self.waiting.remote_page() {
            return Some(PageOf::Remote(serial));
        }
        match &self.run {
            Some(run) => Some(PageOf::Run(run.serial())),
            None => config.url.clone().map(PageOf::Override),
        }
    }

    /// The origin a load of the current page is compared with. `None` when
    /// the page failed or was made for another source.
    #[cfg(embedded_pages)]
    pub(crate) fn page_origin(&self, config: &WalkrConfig) -> Option<crate::browser::WebUrl> {
        if !self.page_open() || self.page_for != self.source(config) {
            return None;
        }
        if let Some((page, _)) = self.waiting.remote_page() {
            return Some(page.origin().clone());
        }
        match &self.run {
            Some(run) => Some(run.origin().clone()),
            None => config.url.clone(),
        }
    }

    /// Whether `url` is on the server of the run.
    #[cfg(embedded_pages)]
    pub(crate) fn runs_on(&self, url: &crate::browser::WebUrl) -> bool {
        self.run
            .as_ref()
            .is_some_and(|run| run.origin().origin() == url.origin())
    }

    /// Whether the page shows. A run's page hides while its probe fails:
    /// the server is gone, and the page would only show its error.
    #[cfg(embedded_pages)]
    pub(crate) fn page_visible(&self) -> bool {
        (self.waiting.remote_page().is_some()
            || self.run.is_none()
            || !section::failing(self.connectivity()))
            && !self.detail_shown()
    }

    /// Close, or the idle limit: the page is gone and none starts by itself.
    /// An ended run goes with its page. The caller closed the page itself.
    #[cfg(embedded_pages)]
    pub(crate) fn dismiss(&mut self) {
        let ended = self.run.as_ref().is_some_and(|run| !run.live());
        // A remote page goes with its tunnel.
        self.waiting.close_remote();
        self.page_closed();
        self.dismissed = !ended;
    }

    /// Whether Show page has something to do.
    #[cfg(embedded_pages)]
    pub(crate) fn can_show_page(&self) -> bool {
        self.dismissed && self.run.as_ref().is_some_and(Run::live)
    }

    /// Show page: a page may start again.
    #[cfg(embedded_pages)]
    pub(crate) fn show_page(&mut self, now: Instant) {
        self.dismissed = false;
        self.page_requested = true;
        self.active(now);
    }

    pub(crate) fn connectivity(&self) -> Connectivity {
        probe::connectivity(
            self.configured(),
            self.last.as_ref().map(|last| last.reach),
            &self.page,
        )
    }

    /// What the header says: the state, or the host of a page that left Walkr.
    fn label(&self) -> header::Label {
        let state = self.connectivity();
        if let (Connectivity::Reachable, Page::Elsewhere(host)) = (state, &self.page) {
            return header::Label::OtherPage(host.as_str().into());
        }
        match &self.run {
            None => header::Label::State(state),
            Some(_) if section::failing(state) => header::Label::NoAnswer,
            Some(run) if run.live() => header::Label::Live,
            Some(_) => header::Label::Ended,
        }
    }

    #[cfg(embedded_pages)]
    pub(crate) fn page_loaded(&mut self, loaded: &str, configured: &crate::browser::WebUrl) {
        let Ok(loaded) = crate::browser::WebUrl::try_from(loaded) else {
            return;
        };
        self.page = if loaded.origin() == configured.origin() {
            Page::OnWalkr
        } else {
            // A host name is ASCII and at most 253 bytes; a page chooses this one.
            let host = loaded.host();
            Page::Elsewhere(host.get(..HOST_LIMIT).unwrap_or(host).to_owned())
        };
        self.header.update(self.label());
    }

    #[cfg(embedded_pages)]
    pub(crate) fn page_closed(&mut self) {
        self.page = Page::Unseen;
        self.page_for = None;
        self.page_error = None;
        self.settle();
        self.header.update(self.label());
    }

    #[cfg(embedded_pages)]
    pub(crate) fn can_start_page(&self, config: &WalkrConfig) -> bool {
        self.page_requested
            && !self.dismissed
            && self.config == *config
            && self.page_for.is_none()
            && self.page_error.is_none()
            // A remote page has no probe: its address was just read.
            && (self.waiting.remote_page().is_some() || section::can_open(self.connectivity()))
    }

    pub(crate) fn age(&self, config: &WalkrConfig, now: Instant) -> Option<Duration> {
        if self.config != *config {
            return None;
        }
        self.last
            .as_ref()
            .map(|last| now.saturating_duration_since(last.checked))
    }

    pub(crate) fn error(&self) -> Option<&Error> {
        self.start_error.as_ref()
    }
}
