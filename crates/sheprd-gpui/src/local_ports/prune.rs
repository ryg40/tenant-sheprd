//! "Stop orphaned servers...": the servers on this computer whose panes are
//! closed (`listening_ports::Known`), stopped in one batch through the
//! Ports section's own question and Stop path. Asking waits for a listing
//! begun after the request, so every row it shows was just read with its
//! process's start time, which the worker checks again before each signal.
//! Nothing is signalled without the user's confirmation.
// i18n: migrated

use super::{Batch, DEMAND, Listener, LocalPorts, Pending};
use crate::{
    SheprdWindow,
    i18n::{Localize, fl},
    listening_ports::{Daemon, Known},
    usage::Host,
    window::Flash,
};
use gpui::{Context, Window};
use sheprd_control_protocol::protocol::hosts::{LOCAL_PORTS_WAIT_SECONDS, PortAction};
use std::time::{Duration, Instant};

/// One workspace of one daemon, whose orphaned servers alone are stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Scope {
    pub daemon: Daemon,
    pub workspace: String,
}

/// A "Stop orphaned servers" waiting for a fresh listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Prune {
    requested: Instant,
    /// `None` for every workspace of every daemon the window shows.
    scope: Option<Scope>,
}

/// The rows of `listing` whose servers outlived their panes, by `known`,
/// within `scope`.
pub(crate) fn orphaned(
    listing: &[Listener],
    known: &Known,
    scope: Option<&Scope>,
) -> Vec<Listener> {
    listing
        .iter()
        .filter(|listener| listener.orphaned(known))
        .filter(|listener| {
            let (Some(scope), Some(started)) = (scope, &listener.started) else {
                return scope.is_none();
            };
            started.workspace == scope.workspace && scope.daemon.owns(&started.socket)
        })
        .cloned()
        .collect()
}

impl LocalPorts {
    /// Starts a prune of `scope`: keeps the scan running and asks for a
    /// listing now. A newer request replaces an unanswered one.
    pub(crate) fn request_prune(&mut self, scope: Option<Scope>, now: Instant) {
        self.demand(now + DEMAND);
        self.retry();
        self.prune = Some(Prune {
            requested: now,
            scope,
        });
    }

    /// Whether a prune waits for its listing.
    pub(crate) fn pruning(&self) -> bool {
        self.prune.is_some()
    }
}

/// What a prune request comes to, once its listing is in.
enum Answer {
    Ask(Pending),
    Nothing,
    Failed(String),
}

impl SheprdWindow {
    /// The scope of "Stop orphaned servers" for `workspace` of the selected
    /// endpoint: `None` unless it is this computer's and one of its ports
    /// outlived its pane. Servers of a remote host are only marked: their
    /// processes cannot be held by start time over its shell.
    pub(crate) fn orphan_scope(&self, workspace: &str) -> Option<Scope> {
        let endpoint = self.endpoints.get(self.selected_endpoint)?;
        let daemon = Daemon::of(&endpoint.connection.target)?;
        if *daemon.host() != Host::Local || !super::SUPPORTED {
            return None;
        }
        let listed = self.listening_ports.get(&daemon, workspace)?;
        listed
            .ports
            .iter()
            .any(|port| listed.orphaned(port))
            .then(|| Scope {
                daemon,
                workspace: workspace.to_owned(),
            })
    }

    /// "Stop orphaned servers...": asks once a fresh listing is in.
    pub(crate) fn stop_orphaned_servers(&mut self, scope: Option<Scope>, cx: &mut Context<Self>) {
        self.local_ports.request_prune(scope, Instant::now());
        cx.notify();
    }

    /// Answers a waiting prune once a listing begun after it is in: the
    /// question, or a note that there is nothing to stop.
    pub(crate) fn answer_prune(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prune) = self.local_ports.prune.clone() else {
            return;
        };
        let answer = match self.local_ports.fresh_listing(prune.requested) {
            None if prune.requested.elapsed() < Duration::from_secs(LOCAL_PORTS_WAIT_SECONDS) => {
                return;
            }
            None => Answer::Failed(fl!("local-ports-prune-timeout")),
            // The section's own words for it, in the app's language.
            Some(Err(_)) => Answer::Failed(self.local_ports.error().map_or_else(
                || crate::Error::from(super::Error::Stopped).localize(),
                str::to_owned,
            )),
            Some(Ok(listing)) => {
                let listeners = orphaned(listing, &self.known_panes(), prune.scope.as_ref());
                if listeners.is_empty() {
                    Answer::Nothing
                } else {
                    Answer::Ask(Pending {
                        action: PortAction::Stop,
                        batch: Batch::Orphaned,
                        listeners,
                    })
                }
            }
        };
        self.local_ports.prune = None;
        match answer {
            Answer::Ask(pending) => {
                if !self.ask_to_signal_ports(pending, window, cx) {
                    self.show_flash(Flash::warning(fl!("local-ports-prune-busy")), cx);
                }
            }
            Answer::Nothing => self.show_flash(Flash::success(fl!("local-ports-prune-none")), cx),
            Answer::Failed(error) => self.show_flash(
                Flash::warning(fl!("local-ports-prune-failed", error = error)),
                cx,
            ),
        }
        cx.notify();
    }

    /// Marks the ports and Ports rows whose servers outlived their panes.
    /// Whether any mark changed.
    pub(crate) fn update_orphans(&mut self) -> bool {
        // Every frame: nothing is built unless a scan, a listing, or an
        // endpoint changed.
        if self.local_ports.classified() && self.listening_ports.classified(self.orphan_endpoints())
        {
            return false;
        }
        let known = self.known_panes();
        let chips = self.listening_ports.classify(&known);
        let rows = self.local_ports.classify(&known);
        chips || rows
    }
}
