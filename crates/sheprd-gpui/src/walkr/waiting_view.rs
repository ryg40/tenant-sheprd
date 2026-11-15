//! The waiting count of each counted host above the saved sessions, a
//! host's failure, the waiting sessions of SSH hosts with the state of the
//! page of the picked one, and what the last click of Walkr's icon could
//! not do. Drawn from the last answers only. A session of this computer
//! shows the state of its page in its own row of the list.
//!
//! **Clear** above the hosts takes every waiting session out of the count
//! and the list, and the button of a waiting row of an SSH host takes that
//! one; Sheprd alone forgets them ([`super::waiting::Dismissed`]).
// i18n: migrated
use super::{
    history_view::local_time,
    waiting::{Error, HostWaiting, RemoteState},
};
use crate::{SheprdWindow, automations::Machine, config::corners, direction::Logical, i18n::fl};
use gpui::{prelude::*, *};

/// A tooltip of `text` in the theme's colors.
fn hint(
    text: SharedString,
    foreground: u32,
    surface: u32,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |_, cx| {
        let text = text.clone();
        cx.new(|_| crate::usage::Hint {
            text,
            foreground,
            surface,
        })
        .into()
    }
}

fn host_line(host: &HostWaiting) -> SharedString {
    let label = host.label.as_str();
    match (&host.error, &host.tally) {
        (Some(Error::TimedOut), _) => fl!("walkr-waiting-host-timed-out", host = label),
        (Some(Error::NoPython), _) => fl!("walkr-waiting-host-no-python", host = label),
        (Some(_), _) => fl!("walkr-waiting-host-failed", host = label),
        (None, Some(tally)) => fl!("walkr-waiting-host", host = label, count = tally.waiting),
        (None, None) => fl!("walkr-waiting-host-reading", host = label),
    }
    .into()
}

/// What the page of a picked waiting session of `host` does. Only a page
/// of an SSH host goes through a tunnel; the failures are the same.
pub(super) fn remote_line(state: &RemoteState, host: &str, local: bool) -> SharedString {
    match state {
        RemoteState::Opening if local => fl!("walkr-local-opening", host = host),
        RemoteState::Open(_) if local => fl!("walkr-local-open", host = host),
        RemoteState::Opening => fl!("walkr-remote-opening", host = host),
        RemoteState::Open(_) => fl!("walkr-remote-open", host = host),
        #[cfg(windows)]
        RemoteState::Failed(Error::LocalUnsupported) => fl!("walkr-local-unsupported", host = host),
        RemoteState::Failed(Error::NoPages) => fl!("walkr-remote-no-pages"),
        RemoteState::Failed(Error::NoPython) => fl!("walkr-remote-no-python", host = host),
        RemoteState::Failed(Error::NoServer) => fl!("walkr-remote-no-server", host = host),
        RemoteState::Failed(Error::OtherServer) => fl!("walkr-remote-other-server", host = host),
        RemoteState::Failed(Error::PortBusy(port)) => {
            fl!(
                "walkr-remote-port-busy",
                host = host,
                port = port.to_string()
            )
        }
        RemoteState::Failed(Error::ServerStopped) => fl!("walkr-remote-stopped", host = host),
        RemoteState::Failed(Error::TimedOut) => fl!("walkr-remote-timed-out", host = host),
        RemoteState::Failed(Error::Tunnel(_)) => fl!("walkr-remote-tunnel", host = host),
        RemoteState::Failed(_) => fl!("walkr-remote-failed", host = host),
    }
    .into()
}

impl SheprdWindow {
    /// The line above the hosts while a page waits: the total and **Clear**.
    fn walkr_waiting_header(&self, total: usize, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        let clear = self
            .walkr_button("walkr-clear", fl!("walkr-clear").into(), true)
            .tooltip(hint(
                fl!("walkr-clear-hint").into(),
                theme.foreground,
                theme.surface,
            ))
            .on_click(cx.listener(|this, _, _, cx| {
                this.clear_walkr_waiting();
                cx.notify();
            }));
        div()
            .debug_selector(|| "walkr-waiting-header".into())
            .px_3()
            .py_1()
            .row()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(self.config.ui.size - 1.))
                    .text_color(rgb(theme.muted))
                    .child(fl!("walkr-waiting-total", count = total)),
            )
            .child(clear)
    }

    /// The button that takes waiting session `id` of `machine`, titled
    /// `title`, out of the count and the list.
    fn walkr_dismiss_button(
        &self,
        at: usize,
        machine: Machine,
        id: String,
        title: &str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = &self.theme;
        div()
            .id(("walkr-remote-dismiss", at))
            .debug_selector(move || format!("walkr-remote-dismiss-{at}"))
            .role(Role::Button)
            .aria_label(fl!("walkr-dismiss", title = title))
            .flex_none()
            .size(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(corners::CONTROL))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(theme.active)))
            .tooltip(hint(
                fl!("walkr-dismiss-hint").into(),
                theme.foreground,
                theme.surface,
            ))
            .child(
                svg()
                    .path("icons/x.svg")
                    .size(px(12.))
                    .text_color(rgb(theme.muted)),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.dismiss_walkr_waiting(&machine, &id);
                cx.notify();
            }))
    }

    /// The row of the live Walkr server of SSH host `host`, and the state of
    /// its page once picked. A pick opens the server's Compare page, which
    /// is not a waiting page and is not counted.
    fn walkr_live_server(
        &self,
        host_index: usize,
        host: &HostWaiting,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = &self.theme;
        let small = px(self.config.ui.size - 1.);
        let picked = self
            .walkr
            .waiting
            .remote
            .as_ref()
            .filter(|remote| remote.machine == host.machine && remote.id.is_none());
        let machine = host.machine.clone();
        let row = div()
            .id(("walkr-live-server", host_index))
            .debug_selector(move || format!("walkr-live-server-{host_index}"))
            .role(Role::Button)
            .aria_label(fl!("walkr-live-server"))
            .px_3()
            .py_1()
            .flex()
            .flex_col()
            .cursor_pointer()
            .when(picked.is_some(), |row| row.bg(rgb(theme.active)))
            .hover(|style| style.bg(rgb(theme.active)))
            .child(div().min_w_0().truncate().child(fl!("walkr-live-server")))
            .child(
                div()
                    .text_size(small)
                    .text_color(rgb(theme.muted))
                    .child(fl!("walkr-live-server-open")),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.walkr.waiting.unmatched = false;
                this.walkr.sessions.back();
                this.open_waiting_walkr_page(machine.clone(), None, window, cx);
                cx.notify();
            }));
        let state = picked.map(|remote| {
            div()
                .debug_selector(|| "walkr-waiting-remote".into())
                .px_3()
                .py_1()
                .text_size(small)
                .text_color(rgb(theme.muted))
                .child(remote_line(&remote.state, host.label.as_str(), false))
        });
        div().flex().flex_col().child(row).children(state)
    }

    pub(super) fn walkr_waiting(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        let waiting = &self.walkr.waiting;
        let small = px(self.config.ui.size - 1.);
        let mut block = div()
            .debug_selector(|| "walkr-waiting".into())
            .flex()
            .flex_col()
            .pb_1();
        let total = waiting.total();
        if total > 0 {
            block = block.child(self.walkr_waiting_header(total, cx));
        }
        if waiting.unmatched {
            block = block.child(
                div()
                    .debug_selector(|| "walkr-waiting-unmatched".into())
                    .px_3()
                    .py_1()
                    .text_color(rgb(theme.foreground))
                    .child(fl!("walkr-waiting-unmatched")),
            );
        }
        for (host_index, host) in waiting.hosts().iter().enumerate() {
            block = block.child(
                div()
                    .debug_selector(move || format!("walkr-waiting-host-{host_index}"))
                    .px_3()
                    .py_0p5()
                    .text_size(small)
                    .text_color(rgb(theme.muted))
                    .child(host_line(host)),
            );
            // This computer's sessions are the list below.
            let Some(tally) = host.tally.as_ref().filter(|_| !host.machine.is_local()) else {
                continue;
            };
            if tally.live {
                block = block.child(self.walkr_live_server(host_index, host, cx));
            }
            for (index, pending) in tally.newest.iter().enumerate() {
                let picked = waiting.remote.as_ref().filter(|remote| {
                    remote.machine == host.machine
                        && remote.id.as_deref() == Some(pending.id.as_str())
                });
                let title: SharedString = pending
                    .title
                    .clone()
                    .filter(|title| !title.trim().is_empty())
                    .map_or_else(|| fl!("walkr-row-untitled").into(), SharedString::from);
                let pick = (host.machine.clone(), pending.id.clone());
                let at = host_index * 1_000 + index;
                let dismiss = self.walkr_dismiss_button(
                    at,
                    host.machine.clone(),
                    pending.id.clone(),
                    &title,
                    cx,
                );
                let row = div()
                    .id(("walkr-remote-row", at))
                    .debug_selector(move || format!("walkr-remote-row-{host_index}-{index}"))
                    .role(Role::Button)
                    .aria_label(title.clone())
                    .flex_1()
                    .min_w_0()
                    .px_3()
                    .py_1()
                    .flex()
                    .flex_col()
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(theme.active)))
                    .child(div().min_w_0().truncate().child(title))
                    .child(
                        div()
                            .row()
                            .gap_2()
                            .text_size(small)
                            .text_color(rgb(theme.muted))
                            .child(local_time(pending.created))
                            .child(fl!("walkr-row-open")),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let (machine, id) = pick.clone();
                        this.walkr.waiting.unmatched = false;
                        this.walkr.sessions.back();
                        this.open_waiting_walkr_page(machine, Some(id), window, cx);
                        cx.notify();
                    }));
                // The dismiss button sits beside the row, not in it, so its
                // click picks no page.
                block = block.child(
                    div()
                        .row()
                        .items_center()
                        .pr_2()
                        .when(picked.is_some(), |line| line.bg(rgb(theme.active)))
                        .child(row)
                        .child(dismiss),
                );
                if let Some(remote) = picked {
                    block = block.child(
                        div()
                            .debug_selector(|| "walkr-waiting-remote".into())
                            .px_3()
                            .py_1()
                            .text_size(small)
                            .text_color(rgb(theme.muted))
                            .child(remote_line(&remote.state, host.label.as_str(), false)),
                    );
                }
            }
        }
        block
    }
}
