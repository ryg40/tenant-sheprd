//! Asks before removing what sites keep in Servo's page tabs from one run
//! to the next: cookies and site storage. `sheprd browser
//! clear-data` only opens this question; nothing goes until the user
//! confirms here, and every site is signed out then.
// i18n: migrated

#[cfg(servo_pages)]
use super::Page;
use crate::SheprdWindow;
#[cfg(servo_pages)]
use crate::{
    browser::engine::{self, Cleared},
    i18n::{Localize, fl},
    window::Flash,
};
use gpui::{prelude::*, *};

impl SheprdWindow {
    /// Whether the question is open, for the menu's layout.
    pub(in crate::menu) fn browser_data_open(&self) -> bool {
        #[cfg(servo_pages)]
        return self.menu.page == Some(Page::BrowserData);
        #[cfg(not(servo_pages))]
        false
    }

    /// The open question's body. Only Servo's pages keep their data in a
    /// folder of the app, so other builds never ask.
    pub(in crate::menu) fn browser_data_page(&self, cx: &mut Context<Self>) -> Option<Div> {
        #[cfg(servo_pages)]
        return (self.menu.page == Some(Page::BrowserData)).then(|| self.render_browser_data(cx));
        #[cfg(not(servo_pages))]
        {
            let _ = cx;
            None
        }
    }
}

#[cfg(servo_pages)]
impl SheprdWindow {
    /// Shows the question, unless another page holds the window; whether it
    /// is shown.
    pub(crate) fn ask_to_clear_browser_data(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.menu.page.is_some() || !self.open_menu(window, cx) {
            return false;
        }
        self.menu.page = Some(Page::BrowserData);
        cx.notify();
        true
    }

    /// Removes the data, and closes the question. The folder is emptied on
    /// a background thread; the window says how it went, and when open
    /// pages still hold data, that a restart completes the removal.
    pub(in crate::menu) fn confirm_clear_browser_data(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.menu.page != Some(Page::BrowserData) {
            return;
        }
        self.dismiss_menu(window, cx);
        let folder = engine::DataFolder::of(cx);
        let cleared = cx
            .background_executor()
            .spawn(async move { engine::clear_data(folder.as_deref()) });
        cx.spawn(async move |this, cx| {
            let cleared = cleared.await;
            if let Ok(cleared) = &cleared {
                tracing::info!(?cleared, "Cleared the data of browser tabs");
            }
            let flash = match cleared {
                Ok(Cleared::Now) => Flash::success(fl!("browser-data-cleared")),
                Ok(Cleared::AtNextStart) => Flash::success(fl!("browser-data-cleared-running")),
                Err(error) => {
                    tracing::warn!(%error, "Could not clear the data of browser tabs");
                    Flash::warning(error.localize())
                }
            };
            let _ = this.update(cx, |this, cx| this.show_flash(flash, cx));
        })
        .detach();
    }

    fn render_browser_data(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        div()
            .debug_selector(|| "browser-data".into())
            .flex()
            .flex_col()
            .p(px(8.))
            .gap(px(12.))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(fl!("browser-data-title")),
            )
            .child(
                div()
                    .text_color(rgb(theme.subtext()))
                    .child(fl!("browser-data-note")),
            )
            // What the clearing cannot do while pages are open is said
            // before the user confirms, not after.
            .when(engine::holds_data(), |page| {
                page.child(
                    div()
                        .debug_selector(|| "browser-data-open-pages".into())
                        .text_color(rgb(theme.subtext()))
                        .child(fl!("browser-data-note-open-pages")),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("browser-data-cancel")
                            .debug_selector(|| "browser-data-cancel".into())
                            .p(px(6.))
                            .cursor_pointer()
                            .child(fl!("browser-data-cancel"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.dismiss_menu(window, cx);
                            })),
                    )
                    .child(
                        div()
                            .id("browser-data-confirm")
                            .debug_selector(|| "browser-data-confirm".into())
                            .p(px(6.))
                            .rounded(px(crate::config::corners::CONTROL))
                            .bg(rgb(theme.active))
                            .text_color(super::danger(theme))
                            .cursor_pointer()
                            .child(fl!("browser-data-confirm"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.confirm_clear_browser_data(window, cx);
                            })),
                    ),
            )
    }
}
