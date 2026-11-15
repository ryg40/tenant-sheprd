//! The Artifacts button's click, the confirmed start and stop, and opening
//! the page, in a window. Each run of the program holds the app's one job
//! slot, so a second click while one runs starts no second child.
// i18n: migrated
use super::{
    Action, Error, Job, NotAsked, Refusal, Result, Status, claim, decide, program, release,
    same_server,
};
use crate::{
    SheprdWindow,
    browser::WebUrl,
    i18n::{Localize as _, fl},
    window::Flash,
};
use gpui::{Context, SharedString, Window};
use std::path::PathBuf;

impl SheprdWindow {
    /// A click on the Artifacts button. Without the program it opens
    /// `[artifacts] url` as before; with it, `decide` chooses.
    pub(crate) fn click_artifacts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if super::program_path(cx).is_none() {
            if let Some(url) = self.config.artifacts.url.clone() {
                self.open_artifacts_page(url, window, cx);
            }
            return;
        }
        // A setting on another machine is never the program's local server,
        // so it opens at once, without the read.
        if let Some(url) = self.config.artifacts.url.clone()
            && url.loopback_port().is_none()
        {
            self.open_artifacts_page(url, window, cx);
            return;
        }
        self.read_artifacts_status(window, cx, |this, status, window, cx| {
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
                Some(Action::Ask(url)) => {
                    if let Err(not_asked) = this.ask_to_start_artifacts(url, window, cx) {
                        this.flash_artifacts(not_asked_text(not_asked), cx);
                    }
                }
                Some(Action::Refuse(refusal)) => this.flash_artifacts(refused(refusal), cx),
                None => {}
            }
        });
    }

    /// Runs `artifacts --json status` on a background thread, then `then`
    /// in this window. A missing program or a busy slot answers at once.
    pub(crate) fn read_artifacts_status(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Result<Status>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let claimed = super::program_path(cx)
            .ok_or(Error::NoProgram)
            .and_then(|program| self.claim_artifacts(Job::Reading, cx).map(|()| program));
        let program = match claimed {
            Ok(program) => program,
            Err(error) => return then(self, Err(error), window, cx),
        };
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

    /// After the user said yes: runs `program start`, then opens the page
    /// the question showed. When the program printed a page of another
    /// origin, the server is not the one asked about, so nothing opens and
    /// the flash names where it said it started.
    pub(crate) fn start_artifacts(
        &mut self,
        asked: WebUrl,
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
                Ok(Some(printed)) if !same_server(&printed, &asked) => {
                    let text = fl!("artifacts-started-elsewhere", address = printed.address());
                    this.flash_artifacts(text.into(), cx);
                }
                Ok(_) => {
                    this.open_artifacts_page(asked, window, cx);
                }
                Err(error) => this.flash_artifacts(failed(&error), cx),
            });
        })
        .detach();
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
    fn claim_artifacts(&mut self, job: Job, cx: &mut Context<Self>) -> Result<()> {
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
    /// workspace. `false` when no workspace could take it.
    pub(crate) fn open_artifacts_page(
        &mut self,
        url: WebUrl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !crate::browser::EMBEDDED {
            cx.open_url(url.as_str());
            return true;
        }
        let Some((_, workspace)) = self.browser_key() else {
            self.show_flash(Flash::warning(fl!("browser-open-workspace-first")), cx);
            return false;
        };
        let endpoint = self.endpoints[self.selected_endpoint].id.clone();
        self.open_workspace_page(&endpoint, &workspace, url, window, cx);
        true
    }

    fn flash_artifacts(&mut self, text: SharedString, cx: &mut Context<Self>) {
        self.show_flash(Flash::warning(text), cx);
    }
}

/// Frees the job slot, also when the window that ran the job has closed.
fn finish(cx: &gpui::AsyncApp) {
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

/// Why the question did not open; a program that is gone is the same as one
/// that is not installed.
pub(crate) fn not_asked_text(not_asked: NotAsked) -> SharedString {
    match not_asked {
        NotAsked::DialogOpen => fl!("artifacts-ask-busy").into(),
        NotAsked::NoProgram => failed(&Error::NoProgram),
    }
}

/// A short reason for a flash.
pub(crate) fn failed(error: &Error) -> SharedString {
    error.localize().into()
}
