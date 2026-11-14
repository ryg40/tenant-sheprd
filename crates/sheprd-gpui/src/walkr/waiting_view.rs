//! The waiting count of each counted host above the saved sessions, a
//! host's failure, the waiting sessions of SSH hosts with the state of the
//! page of the picked one, and what the last click of Walkr's icon could
//! not do. Drawn from the last answers only. A session of this computer
//! shows the state of its page in its own row of the list.
// i18n: migrated
use super::{
    history_view::local_time,
    waiting::{Error, HostWaiting, RemoteState},
};
use crate::{SheprdWindow, direction::Logical, i18n::fl};
use gpui::{prelude::*, *};

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
    pub(super) fn walkr_waiting(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        let waiting = &self.walkr.waiting;
        let small = px(self.config.ui.size - 1.);
        let mut block = div()
            .debug_selector(|| "walkr-waiting".into())
            .flex()
            .flex_col()
            .pb_1();
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
            for (index, pending) in tally.newest.iter().enumerate() {
                let picked = waiting
                    .remote
                    .as_ref()
                    .filter(|remote| remote.machine == host.machine && remote.id == pending.id);
                let title: SharedString = pending
                    .title
                    .clone()
                    .filter(|title| !title.trim().is_empty())
                    .map_or_else(|| fl!("walkr-row-untitled").into(), SharedString::from);
                let pick = (host.machine.clone(), pending.id.clone());
                let row = div()
                    .id(("walkr-remote-row", host_index * 1_000 + index))
                    .debug_selector(move || format!("walkr-remote-row-{host_index}-{index}"))
                    .role(Role::Button)
                    .aria_label(title.clone())
                    .px_3()
                    .py_1()
                    .flex()
                    .flex_col()
                    .cursor_pointer()
                    .when(picked.is_some(), |row| row.bg(rgb(theme.active)))
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
                        this.open_waiting_walkr_page(machine, id, window, cx);
                        cx.notify();
                    }));
                block = block.child(row);
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
