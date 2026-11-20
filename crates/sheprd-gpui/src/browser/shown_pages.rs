//! The page of a tab a group shows. A tab gets its page when someone
//! shows it, and a tab nobody showed since the start has none: no web
//! view, and no request to its site. A layout restored at the start shows
//! a tab that nobody asked for in this run, so the window makes the page
//! of each tab it draws, on its ticks. A page that failed or
//! stopped stays as its tab's line says, until its reload.
use super::store::Tab;
#[cfg(servo_pages)]
use super::view::store;
use crate::SheprdWindow;
use gpui::{Context, Window};

impl SheprdWindow {
    /// Makes `tab`'s page if the window has none, and keeps why it could
    /// not for the tab to show.
    pub(super) fn make_tab_page(&mut self, tab: &Tab, window: &mut Window, cx: &mut Context<Self>) {
        match self.browser.pages.ensure(tab, window, cx) {
            Ok(()) => {
                self.browser.failed.remove(&tab.id);
            }
            Err(error) => {
                tracing::warn!(%error, "Cannot create a browser page");
                self.browser
                    .failed
                    .insert(tab.id, crate::i18n::Localize::localize(&error).into());
            }
        }
    }

    /// Whether the window draws its groups: no activity bar section or
    /// mod app takes the body in their place.
    #[cfg(embedded_pages)]
    pub(super) fn draws_groups(&self) -> bool {
        self.activity_section().is_none() && self.mod_app().is_none()
    }

    /// Makes the page of each tab the window draws now and has no page
    /// for. What is drawn is what `live_pages` says, which the window
    /// presents: a tab its group shows behind a section, in a group zen
    /// mode hides, or as a file's source gets no page, and no request goes
    /// to its site, until it is drawn. Runs on every window tick, never
    /// from render: a page starts the engine.
    #[cfg(servo_pages)]
    pub(super) fn ensure_shown_pages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = store(cx) else {
            return;
        };
        let wanted: Vec<Tab> = self
            .live_pages(cx)
            .into_iter()
            .filter(|id| {
                !self.browser.pages.contains(*id)
                    && !self.browser.pages.stopped(*id)
                    && !self.browser.failed.contains_key(id)
            })
            .filter_map(|id| store.get(id))
            // An editor's page waits for its server, with its provider.
            .filter(|tab| {
                !tab.place.is_editor() && tab.location.as_ref().is_some_and(|l| l.is_page())
            })
            .cloned()
            .collect();
        if wanted.is_empty() {
            return;
        }
        for tab in &wanted {
            self.make_tab_page(tab, window, cx);
        }
        cx.notify();
    }
}
