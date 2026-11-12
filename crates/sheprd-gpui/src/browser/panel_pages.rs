//! Walkr's section owns one page, outside the saved browser-tab store.
use super::{TabId, page::PageElement};
use crate::{
    SheprdWindow,
    i18n::Localize as _,
    sidebar::ActivityPanel,
    walkr::{PageOf, run::Address},
};
use gpui::{Context, Window};
use std::time::Instant;

impl SheprdWindow {
    /// Makes the page for the run, else for `walkr.url`. A page is made for
    /// one address and never loads another: a newer address of the run gets
    /// a new page. The address of a run goes to the engine in an [`Address`],
    /// whose `Debug` shows host and port only: it reaches no tab, no store,
    /// and no log.
    pub(crate) fn open_walkr_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> crate::Result<()> {
        let Some(source) = self.walkr.source(&self.config.walkr) else {
            return Ok(());
        };
        let address = match (&source, self.walkr.run()) {
            (PageOf::Run(_), Some(run)) => run.address().clone(),
            (PageOf::Override(url), _) => Address::from(url.clone()),
            (PageOf::Run(_), None) => return Ok(()),
        };
        self.browser.pages.ensure_walkr(&address, window, cx)?;
        self.walkr.page_for = Some(source);
        tracing::debug!("Created Walkr section page");
        Ok(())
    }

    pub(crate) fn walkr_page_element(&self) -> Option<PageElement> {
        self.browser.pages.element(TabId::WALKR)
    }

    pub(crate) fn close_walkr_page(&mut self) {
        self.browser.pages.close(TabId::WALKR);
        self.browser.failed.remove(&TabId::WALKR);
        self.walkr.page_closed();
    }

    /// The Close button and the idle limit: an ended run goes with its
    /// page, and a live run or `walkr.url` starts no page by itself.
    pub(crate) fn dismiss_walkr_page(&mut self) {
        self.browser.pages.close(TabId::WALKR);
        self.browser.failed.remove(&TabId::WALKR);
        self.walkr.dismiss();
    }

    pub(crate) fn reload_walkr_page(&self) {
        self.browser.pages.reload(TabId::WALKR);
    }

    pub(crate) fn blur_walkr_page(&self) {
        self.browser.pages.blur(TabId::WALKR);
    }

    pub(crate) fn fail_walkr_page(&mut self, error: gpui::SharedString) {
        let source = self.walkr.page_for.clone();
        // An ended run has no page now, so this forgets it.
        self.close_walkr_page();
        if source.is_some() && source == self.walkr.source(&self.config.walkr) {
            // Remember which source failed, until an explicit retry or a new one.
            self.walkr.page_for = source;
            self.walkr.page_error = Some(error);
        }
    }

    /// A tick, never render, creates a page once the shown section's probe answers.
    /// `now` is passed in so that a test can feed the idle clock.
    pub(crate) fn poll_walkr_page(
        &mut self,
        now: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A change of the source closes the page first.
        if self.walkr.page_for.is_some()
            && self.walkr.page_for != self.walkr.source(&self.config.walkr)
        {
            self.close_walkr_page();
            cx.notify();
            return;
        }
        if self.browser.pages.took_input(TabId::WALKR) {
            self.walkr.active(now);
        }
        if self.walkr.idle(now) {
            tracing::debug!("Closed the idle Walkr section page");
            self.dismiss_walkr_page();
            cx.notify();
            return;
        }
        // Windows finishes construction asynchronously through the tab build path.
        if let Some(error) = self.browser.failed.remove(&TabId::WALKR) {
            self.fail_walkr_page(error);
            cx.notify();
        }
        if self.activity_section() != Some(ActivityPanel::Walkr)
            || self.mod_app().is_some()
            || !self.walkr.can_start_page(&self.config.walkr)
        {
            return;
        }
        if let Err(error) = self.open_walkr_page(window, cx) {
            self.walkr.page_for = self.walkr.source(&self.config.walkr);
            self.fail_walkr_page(error.localize().into());
        }
        cx.notify();
    }
}
