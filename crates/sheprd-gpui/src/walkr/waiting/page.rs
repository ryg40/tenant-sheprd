//! The page of a waiting session, opened in the section: a background task
//! reads its address from the host's Walkr server with the reader of
//! [`remote`]. On this computer the section shows that address; on an SSH
//! host it opens an SSH tunnel from the same port on this computer to that
//! server's loopback port first. Nothing on the host or the server is
//! started, stopped or written. Logs name the server's port only.
use super::{Error, RemoteState, remote};
use crate::{
    SheprdWindow,
    automations::Machine,
    listening_ports::{Key, tunnel},
};
use gpui::{Context, Window};
use std::sync::atomic::AtomicBool;

impl SheprdWindow {
    /// A click on session `id` of this computer, in the list or on Walkr's
    /// icon: it is picked. The live run's session shows the run's page, and
    /// opens it again after Close or the idle limit. With no live run, a
    /// session that `waits` for a pick opens its page from the Walkr server
    /// of this computer; its text shows until then, and when it fails.
    pub(crate) fn pick_local_walkr_session(
        &mut self,
        id: String,
        waits: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A local session takes the place of a remote page.
        self.walkr.waiting.close_remote();
        let live = self.walkr.live_session() == Some(id.as_str());
        let run = self.walkr.run().is_some_and(|run| run.live());
        self.walkr.sessions.choose(id.clone());
        if live {
            #[cfg(embedded_pages)]
            self.show_walkr_page(window, cx);
        } else if waits && !run {
            self.open_waiting_walkr_page(Machine::Local, Some(id), window, cx);
        }
    }

    /// Opens the page of session `id` of `machine`, this computer or an SSH
    /// host, in the section; with no `id`, the Compare page of the host's
    /// live Walkr server. Its state shows in the section while it opens,
    /// and when it fails.
    pub(crate) fn open_waiting_walkr_page(
        &mut self,
        machine: Machine,
        id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let waiting = &mut self.walkr.waiting;
        let target = match &machine {
            Machine::Local => None,
            Machine::Ssh(target) => Some(target.clone()),
            #[cfg(feature = "cloud")]
            Machine::Cloud { .. } => return,
        };
        let Some(host) = waiting.host_of(&machine) else {
            return;
        };
        // Windows runs no reader on this computer: see `Error::LocalUnsupported`.
        #[cfg(windows)]
        if target.is_none() {
            waiting.begin_remote(machine, id, RemoteState::Failed(Error::LocalUnsupported));
            return;
        }
        if !crate::browser::EMBEDDED {
            waiting.begin_remote(machine, id, RemoteState::Failed(Error::NoPages));
            return;
        }
        let serial = waiting.begin_remote(machine, id.clone(), RemoteState::Opening);
        let attempt = tunnel::Attempt::default();
        let task = cx.background_executor().spawn(async move {
            let cancelled = AtomicBool::new(false);
            let page = remote::page(&host, id.as_deref(), remote::DEADLINE, &cancelled)?;
            // This computer's server needs no tunnel.
            let Some(target) = target else {
                tracing::debug!(port = page.port(), "Read the address of a local Walkr page");
                return Ok((page.direct()?, None));
            };
            let key = Key {
                target,
                address: None,
                port: page.port(),
            };
            // The server's own port, or no page: see `Remote::tunneled`.
            let opened = tunnel::open(&key, Some(page.port()), &attempt)
                .map_err(|error| Error::Tunnel(Box::new(error)))?;
            tracing::debug!(port = page.port(), "Opened a tunnel to a remote Walkr page");
            // A tunnel on another port drops here, which ends its `ssh`.
            Ok((page.tunneled(opened.local())?, Some(opened)))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            // A window closed meanwhile drops the result, and so its tunnel.
            let _ = this.update_in(cx, |this, window, cx| {
                let opened = this.walkr.waiting.finish_remote(serial, result);
                #[cfg(embedded_pages)]
                if opened {
                    this.show_walkr_page(window, cx);
                }
                #[cfg(not(embedded_pages))]
                let _ = (opened, window);
                cx.notify();
            });
        })
        .detach();
    }
}
