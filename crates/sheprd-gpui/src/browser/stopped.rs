//! A tab whose page stopped working, as when Servo's thread that ran the
//! page's script ended (`engine::stopped` says how the app learns it).
//! The window drops the page, as for one it could not make, and the tab
//! shows that line with a button that opens the page again. The toolbar's
//! reload and `sheprd browser reload` open it again too, in the window
//! that had it, and choose no tab. An engine that refuses the new page
//! (a full queue) leaves the tab on the line of a page it could not make,
//! and `sheprd browser reload` counts the tab as not reloaded.
use super::TabId;
use crate::SheprdWindow;
use gpui::{Context, Window};
#[cfg(servo_pages)]
use {
    super::groups::Slot,
    crate::i18n::fl,
    gpui::{Div, Stateful, prelude::*},
};

impl SheprdWindow {
    /// The toolbar's reload. A page that stopped opens again at the
    /// address its tab shows; any other page, and a file's source, loads
    /// again where it is.
    pub(super) fn reload_browser_tab(
        &mut self,
        id: TabId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A refusal shows as the tab's failure line.
        if matches!(self.reopen_stopped(id, window, cx), Ok(false)) {
            self.reload_file_view(id, cx);
        }
    }

    /// The button under the line of tab `id`, if its page stopped.
    #[cfg(servo_pages)]
    pub(super) fn stopped_reload(
        &self,
        slot: Slot,
        id: TabId,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        if !self.browser.pages.stopped(id) {
            return None;
        }
        let label = fl!("browser-stopped-reload").into();
        let button = self
            .placeholder_button(slot, "browser-stopped-reload", label)
            .on_click(cx.listener(move |this, _, window, cx| {
                // A refusal shows as the tab's failure line.
                let _ = this.reopen_stopped(id, window, cx);
            }));
        Some(button)
    }

    /// Opens tab `id`'s page again at the address the tab shows, in a new
    /// view, if its page stopped in this window: the old view's script
    /// thread is gone. No tab is chosen: a group that shows the tab shows
    /// the new page. Whether the page had stopped, or the error of an
    /// engine that refused the new page, which the tab then shows.
    pub(super) fn reopen_stopped(
        &mut self,
        id: TabId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> crate::Result<bool> {
        #[cfg(servo_pages)]
        {
            if !self.browser.pages.stopped(id) {
                return Ok(false);
            }
            let Some(tab) = super::view::store(cx)
                .and_then(|store| store.get(id))
                .cloned()
            else {
                return Ok(false);
            };
            self.browser.blank.forget(id);
            let made = self.browser.pages.ensure(&tab, window, cx);
            match &made {
                Ok(()) => {
                    self.browser.failed.remove(&id);
                }
                Err(error) => {
                    tracing::warn!(%error, "Cannot create a browser page");
                    self.browser
                        .failed
                        .insert(id, crate::i18n::Localize::localize(error).into());
                }
            }
            cx.notify();
            made.map(|()| true)
        }
        #[cfg(not(servo_pages))]
        {
            let _ = (id, window, cx);
            Ok(false)
        }
    }
}
