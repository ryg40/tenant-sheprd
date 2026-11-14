//! What a window does with its run: `walkr connect`, `walkr disconnect`,
//! and panes that their daemon no longer lists.
use super::run::{Connected, Disconnected, Pane, Parsed};
use crate::{SheprdWindow, sidebar::ActivityPanel};
use gpui::{Context, Window};
use std::time::Instant;

impl SheprdWindow {
    /// The pane holds a Walkr server at `parsed`. A run of another server
    /// gives way, and its page closes here.
    pub(crate) fn connect_walkr(
        &mut self,
        parsed: Parsed,
        pane: Pane,
        show: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Connected, super::Error> {
        let connected = self.walkr.connect(parsed, pane, Instant::now())?;
        #[cfg(embedded_pages)]
        if self.walkr.page_for.is_some()
            && (self.walkr.page_for != self.walkr.source(&self.config.walkr)
                || self.walkr.page_error.is_some())
        {
            // The page of another source, or one that failed: the next tick
            // makes the page of this run.
            self.close_walkr_page();
        } else if connected.moved && self.walkr.page_open() {
            // A page is made for one address. The page of the address before
            // this one is retired, built or still being built, so that no
            // report of its load counts for the new one.
            self.close_walkr_page();
            if let Err(error) = self.open_walkr_page(window, cx) {
                self.walkr.page_for = self.walkr.source(&self.config.walkr);
                self.fail_walkr_page(crate::i18n::Localize::localize(&error).into());
            }
        }
        if show {
            self.show_activity_panel(ActivityPanel::Walkr, window, cx);
        }
        cx.notify();
        Ok(connected)
    }

    pub(crate) fn disconnect_walkr(&mut self, pane: &Pane, cx: &mut Context<Self>) -> Disconnected {
        let disconnected = self.walkr.disconnect(pane);
        if disconnected != Disconnected::NotHeld {
            cx.notify();
        }
        disconnected
    }

    /// A pane that left its daemon's snapshot counts as its `disconnect`.
    /// As for the tabs of a closed workspace, only a snapshot of the same
    /// boot that listed the pane before proves that it is gone. Whether the
    /// run changed.
    pub(crate) fn forget_walkr_panes(&mut self) -> bool {
        if !self.walkr.run().is_some_and(super::run::Run::live) {
            return false;
        }
        // Each daemon this window knows, with what it lists now.
        let daemons: Vec<_> = self
            .endpoints
            .iter()
            .enumerate()
            .filter_map(|(index, endpoint)| {
                let selected = index == self.selected_endpoint;
                let live = if selected { &self.live } else { &endpoint.live };
                let snapshot = live.snapshot.clone()?;
                let socket = endpoint.connection.target.socket_path().ok();
                Some((selected, socket, snapshot))
            })
            .collect();
        self.walkr.observe_panes(|pane| {
            let (_, _, snapshot) = daemons.iter().find(|(selected, socket, _)| {
                match &pane.daemon {
                    Some(daemon) => socket.as_deref() == Some(std::path::Path::new(daemon)),
                    // A caller that names no daemon is in the shown one.
                    None => *selected,
                }
            })?;
            let listed = snapshot
                .panes
                .iter()
                .any(|listed| listed.pane_id == pane.id);
            Some((snapshot.boot_id.as_str(), listed))
        })
    }
}
