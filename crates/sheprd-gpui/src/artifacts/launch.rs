//! The Artifacts button's click, the start, the restart, and the confirmed
//! stop, and opening the page, in a window. A click starts a stopped local
//! server at once, and restarts one that holds on without an answer (`off`,
//! then the start); `sheprd artifacts start` reaches either only through
//! the question's yes. A start first takes its page's port back
//! (`port.rs`), so the program's `start` never has to refuse a taken port.
//! Each run of the program holds the app's one job slot, so a second click
//! while one runs starts no second child. A start runs only the file that
//! answered the read or that the question named, and only while the window
//! can show the page.
// i18n: migrated
use super::{
    Action, Error, Freeing, Job, Refusal, Result, Status, claim, decide, port, program, release,
    same_server,
};
use crate::{
    SheprdWindow,
    browser::{PageOpened, WebUrl},
    i18n::{Localize as _, fl},
    window::Flash,
};
use gpui::{Context, SharedString, Window};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// Why the window cannot show a page of the server now, on a build that
/// embeds pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Blocked {
    /// No focused workspace takes its tab.
    NoWorkspace,
    /// A menu page is open, and the window goes to no workspace while it
    /// is (`navigate_endpoint`).
    DialogOpen,
}

impl Blocked {
    /// Why nothing started, before a start.
    fn text(self) -> SharedString {
        match self {
            Self::NoWorkspace => fl!("browser-open-workspace-first"),
            Self::DialogOpen => fl!("link-refused-dialog-open"),
        }
        .into()
    }

    /// The `$reason` of `artifacts-started-not-shown`, after a start.
    fn selector(self) -> &'static str {
        match self {
            Self::NoWorkspace => "no-workspace",
            Self::DialogOpen => "dialog-open",
        }
    }
}

impl SheprdWindow {
    /// A click on the Artifacts button. Without the program it opens
    /// `[artifacts] url` as before; with it, `decide` chooses, and a stopped
    /// local server starts without a question.
    pub(crate) fn click_artifacts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(program) = super::program_path(cx) else {
            if let Some(url) = self.config.artifacts.url.clone() {
                self.open_artifacts_page(url, window, cx);
            }
            return;
        };
        // A setting on another machine is never the program's local server,
        // so it opens at once, without the read.
        if let Some(url) = self.config.artifacts.url.clone()
            && url.loopback_port().is_none()
        {
            self.open_artifacts_page(url, window, cx);
            return;
        }
        // The read and the start run this one file: a config reload in
        // between starts nothing (`start_artifacts`).
        let read = program.clone();
        self.read_artifacts_status(read, window, cx, move |this, status, window, cx| {
            // A run that holds the slot is not a failed read: say so only.
            if let Err(error @ Error::Busy) = &status {
                return this.flash_artifacts(failed(error), cx);
            }
            let action = decide(status.as_ref().ok(), &this.config.artifacts);
            if let Err(error) = &status {
                this.flash_artifacts(failed(error), cx);
            }
            match action {
                Some(Action::Open(url)) => {
                    this.open_artifacts_page(url, window, cx);
                }
                Some(Action::Start(url)) => this.start_artifacts(url, program, window, cx),
                Some(Action::Restart(url)) => this.restart_artifacts(url, program, window, cx),
                Some(Action::Refuse(refusal)) => this.flash_artifacts(refused(refusal), cx),
                None => {}
            }
        });
    }

    /// Runs `program --json status` on a background thread, then `then`
    /// in this window. A busy slot answers at once.
    pub(crate) fn read_artifacts_status(
        &mut self,
        program: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Result<Status>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if let Err(error) = self.claim_artifacts(Job::Reading, cx) {
            return then(self, Err(error), window, cx);
        }
        let read = cx
            .background_executor()
            .spawn(async move { program::status(&program) });
        cx.spawn_in(window, async move |this, cx| {
            let status = read.await;
            finish(cx);
            let _ = this.update_in(cx, |this, window, cx| then(this, status, window, cx));
        })
        .detach();
    }

    /// On a click, or after the user said yes to the question: runs
    /// `program start`, then opens `expected`. `program` is the file that
    /// answered the click's read, or that the question named: when the app's
    /// program is another file now or none, as after a config reload,
    /// nothing starts. When the program printed a page of another origin,
    /// the server is not the one `expected` names, so nothing opens and the
    /// flash names where it said it started. A build that embeds pages shows
    /// `expected` as a tab of the focused workspace: while the window cannot
    /// go there, nothing starts. While a program listens on the loopback
    /// port of `expected`, nothing starts until that port is free
    /// (`port.rs`).
    pub(crate) fn start_artifacts(
        &mut self,
        expected: WebUrl,
        program: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_artifacts_then(expected, program, port::Taken::Free, window, cx);
    }

    /// [`Self::start_artifacts`], doing `taken` when a program listens on
    /// the page's port. The app's own forwards on it stop first. The port
    /// check runs on a background thread; the window then checks again that
    /// the start may run, as the window can change during the wait.
    pub(super) fn start_artifacts_then(
        &mut self,
        expected: WebUrl,
        program: PathBuf,
        taken: port::Taken,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.may_start(&program, cx) {
            return;
        }
        let Some(port) = expected.loopback_port() else {
            return self.run_start(expected, program, window, cx);
        };
        if let Err(error) = self.claim_artifacts(Job::Starting, cx) {
            return self.flash_artifacts(failed(&error), cx);
        }
        let wait = if self.stop_own_forwards(port, cx) {
            port::RELEASE
        } else {
            Duration::ZERO
        };
        let probe = cx
            .background_executor()
            .spawn(async move { port::free_within(port, wait) });
        cx.spawn_in(window, async move |this, cx| {
            let free = probe.await;
            finish(cx);
            let _ = this.update_in(cx, |this, window, cx| {
                if !this.may_start(&program, cx) {
                    return;
                }
                if free {
                    return this.run_start(expected, program, window, cx);
                }
                let freeing = Freeing {
                    expected,
                    port,
                    program,
                };
                this.port_taken(freeing, taken, window, cx);
            });
        })
        .detach();
    }

    /// Runs `program start` on a background thread, then opens `expected`
    /// as [`Self::start_artifacts`] says.
    fn run_start(
        &mut self,
        expected: WebUrl,
        program: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.claim_artifacts(Job::Starting, cx) {
            return self.flash_artifacts(failed(&error), cx);
        }
        let start = cx
            .background_executor()
            .spawn(async move { program::start(&program) });
        cx.spawn_in(window, async move |this, cx| {
            let started = start.await;
            finish(cx);
            let _ = this.update_in(cx, |this, window, cx| match started {
                Ok(Some(printed)) if !same_server(&printed, &expected) => {
                    let text = fl!(
                        "artifacts-started-other-address",
                        address = printed.address()
                    );
                    this.flash_artifacts(text.into(), cx);
                }
                Ok(_) => this.open_started_page(expected, window, cx),
                Err(error) => this.flash_artifacts(failed(&error), cx),
            });
        })
        .detach();
    }

    /// On a click, or after the user said yes to the question, for the
    /// program's local server that holds on without an answer: runs
    /// `program off`, then the start of [`Self::start_artifacts`]. The same
    /// checks come first, so nothing stops for a start that cannot run.
    pub(crate) fn restart_artifacts(
        &mut self,
        expected: WebUrl,
        program: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.may_start(&program, cx) {
            return;
        }
        if let Err(error) = self.claim_artifacts(Job::Stopping, cx) {
            return self.flash_artifacts(failed(&error), cx);
        }
        let file = program.clone();
        let off = cx
            .background_executor()
            .spawn(async move { program::off(&file) });
        cx.spawn_in(window, async move |this, cx| {
            let stopped = off.await;
            finish(cx);
            let _ = this.update_in(cx, |this, window, cx| match stopped {
                Ok(()) => this.start_artifacts(expected, program, window, cx),
                Err(error) => this.flash_artifacts(failed(&error), cx),
            });
        })
        .detach();
    }

    /// Whether a start of `program` may run now: it is still the app's
    /// program, and the window can show the page. Flashes why not.
    pub(super) fn may_start(&mut self, program: &Path, cx: &mut Context<Self>) -> bool {
        if super::program_path(cx).as_deref() != Some(program) {
            self.flash_artifacts(failed(&Error::NoProgram), cx);
            return false;
        }
        if let Some(blocked) = self.page_blocked() {
            self.flash_artifacts(blocked.text(), cx);
            return false;
        }
        true
    }

    /// After the user said yes: runs `program off`.
    pub(crate) fn stop_artifacts(
        &mut self,
        program: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.claim_artifacts(Job::Stopping, cx) {
            return self.flash_artifacts(failed(&error), cx);
        }
        let off = cx
            .background_executor()
            .spawn(async move { program::off(&program) });
        cx.spawn_in(window, async move |this, cx| {
            let stopped = off.await;
            finish(cx);
            let _ = this.update_in(cx, |this, _, cx| match stopped {
                Ok(()) => this.show_flash(Flash::success(fl!("artifacts-stopped")), cx),
                Err(error) => this.flash_artifacts(failed(&error), cx),
            });
        })
        .detach();
    }

    /// Claims the app's job slot. `claim` redraws the other windows; this
    /// one runs the handler, so it is notified here for its busy pulse.
    pub(super) fn claim_artifacts(&mut self, job: Job, cx: &mut Context<Self>) -> Result<()> {
        if !claim(job, cx) {
            return Err(Error::Busy);
        }
        cx.notify();
        Ok(())
    }

    /// Opens the page in the focused workspace, reusing its tab on that
    /// address as the ports list does. Builds that embed pages
    /// (`browser::EMBEDDED`: macOS, Windows, and Linux with `servo`) show it
    /// as a tab; the others hand it to the system browser, with or without a
    /// workspace. `None` when no workspace could take it.
    pub(crate) fn open_artifacts_page(
        &mut self,
        url: WebUrl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<PageOpened> {
        if !crate::browser::EMBEDDED {
            cx.open_url(url.as_str());
            return Some(PageOpened::SystemBrowser);
        }
        let Some((_, workspace)) = self.browser_key() else {
            self.show_flash(Flash::warning(fl!("browser-open-workspace-first")), cx);
            return None;
        };
        let endpoint = self.endpoints[self.selected_endpoint].id.clone();
        Some(self.open_workspace_page(&endpoint, &workspace, url, window, cx))
    }

    /// After a start: opens the page, or says that the server runs and why
    /// its page did not show. The user's menu page stays open, and a full
    /// tab strip keeps its own warning.
    fn open_started_page(&mut self, url: WebUrl, window: &mut Window, cx: &mut Context<Self>) {
        let reason = match self.page_blocked() {
            Some(blocked) => blocked.selector(),
            None => match self.open_artifacts_page(url, window, cx) {
                Some(PageOpened::Unreachable) => "unreachable",
                _ => return,
            },
        };
        let text = fl!("artifacts-started-not-shown", reason = reason);
        self.flash_artifacts(text.into(), cx);
    }

    /// What keeps the window from showing a page of the server now. Checked
    /// before a start, so that nothing starts for a page that cannot show.
    fn page_blocked(&self) -> Option<Blocked> {
        if !crate::browser::EMBEDDED {
            return None;
        }
        if self.browser_key().is_none() {
            return Some(Blocked::NoWorkspace);
        }
        self.menu.page.is_some().then_some(Blocked::DialogOpen)
    }

    pub(super) fn flash_artifacts(&mut self, text: SharedString, cx: &mut Context<Self>) {
        self.show_flash(Flash::warning(text), cx);
    }
}

/// Frees the job slot, also when the window that ran the job has closed.
pub(super) fn finish(cx: &gpui::AsyncApp) {
    cx.update(release);
}

pub(crate) fn refused(refusal: Refusal) -> SharedString {
    match refusal {
        Refusal::Unreachable => fl!("artifacts-unreachable"),
        Refusal::Unconfigured => fl!("artifacts-unconfigured"),
        Refusal::NoAddress => fl!("artifacts-no-address"),
    }
    .into()
}

/// A short reason for a flash.
pub(crate) fn failed(error: &Error) -> SharedString {
    error.localize().into()
}
