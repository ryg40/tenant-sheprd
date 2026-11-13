//! Which listeners outlived the pane that started them. A server a pane
//! starts often keeps running after the pane closes, re-parented to the
//! init process, and its environment still names the workspace, so the
//! workspace's ports fill with servers nobody can reach from Herdr.
//!
//! A listener is orphaned only when all of these hold: its process names
//! its pane (`HERDR_PANE_ID`) and that pane's daemon (`HERDR_SOCKET_PATH`);
//! that daemon, one the window shows, answered `pane_not_found` for the
//! pane both without and with the pane's context when its host was last
//! scanned, and the process is not this app's own or its descendant
//! ([`super::scan::COMMAND`]); and the window shows that daemon, connected,
//! with a current snapshot that lists every pane of every workspace and not
//! that one. Herdr gives a pane moved to another workspace a new ID but
//! keeps resolving the old one that its processes carry, so a moved pane's
//! server is found, not orphaned. Anything less certain (no answer, no pane
//! named, another daemon's socket, a daemon not shown, connecting, or
//! between snapshots) is not orphaned.

use super::Daemon;
use crate::{state::ConnectionStatus, usage::Host};
use sheprd_client::{ConnectTarget, protocol::ClientShellSnapshot};
use std::{collections::HashSet, sync::Arc};

/// What one enabled endpoint of the window says about its daemon now.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Endpoint<'a> {
    pub target: &'a ConnectTarget,
    pub status: ConnectionStatus,
    pub snapshot: Option<&'a Arc<ClientShellSnapshot>>,
    /// A newer snapshot is on its way.
    pub pending: bool,
}

/// One daemon the window shows: the endpoint as it was read, and what
/// follows from it.
#[derive(Clone, Debug)]
struct Shown {
    target: ConnectTarget,
    status: ConnectionStatus,
    pending: bool,
    given: Option<Arc<ClientShellSnapshot>>,
    /// `None` for a cloud machine, whose ports are not scanned.
    daemon: Option<Daemon>,
    /// `given` while it is current and accounts for every pane.
    snapshot: Option<Arc<ClientShellSnapshot>>,
}

impl Shown {
    /// Whether it was read from `endpoint` as it is now. A snapshot is
    /// replaced, never changed in place, so its pointer says whether it
    /// changed; the rest follows from these fields.
    fn is(&self, endpoint: &Endpoint<'_>) -> bool {
        self.target == *endpoint.target
            && self.status == endpoint.status
            && self.pending == endpoint.pending
            && match (&self.given, endpoint.snapshot) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

impl PartialEq for Shown {
    fn eq(&self, other: &Self) -> bool {
        self.is(&Endpoint {
            target: &other.target,
            status: other.status,
            snapshot: other.given.as_ref(),
            pending: other.pending,
        })
    }
}

/// A listening process whose daemon answered `pane_not_found` for the pane
/// its environment names, and which is not this app's own.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Closed {
    pub host: Host,
    pub pane: String,
    pub pid: u32,
}

/// The daemons a window shows, with the panes each has now, and the
/// processes whose panes their daemons say are closed.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Known {
    closed: HashSet<Closed>,
    shown: Vec<Shown>,
}

impl Known {
    /// Adds a daemon the window shows. Its panes count only while it is
    /// `Connected`, no newer snapshot is pending, and its snapshot lists a
    /// pane for every workspace and a workspace for every pane.
    pub(crate) fn add(&mut self, endpoint: Endpoint<'_>) {
        let current = endpoint.status == ConnectionStatus::Connected && !endpoint.pending;
        let snapshot = endpoint
            .snapshot
            .filter(|snapshot| current && complete(snapshot))
            .cloned();
        self.shown.push(Shown {
            target: endpoint.target.clone(),
            status: endpoint.status,
            pending: endpoint.pending,
            given: endpoint.snapshot.cloned(),
            daemon: Daemon::of(endpoint.target),
            snapshot,
        });
    }

    /// Adds a process whose daemon said its pane is closed.
    pub(crate) fn close(&mut self, closed: Closed) {
        self.closed.insert(closed);
    }

    /// Whether it was built from exactly `endpoints`, as they are now, so
    /// nothing but a new scan could change what it says.
    pub(crate) fn same<'a>(&self, endpoints: impl IntoIterator<Item = Endpoint<'a>>) -> bool {
        let mut shown = self.shown.iter();
        endpoints
            .into_iter()
            .all(|endpoint| shown.next().is_some_and(|known| known.is(&endpoint)))
            && shown.next().is_none()
    }

    /// Whether a listener on `host`, started in `pane` of the daemon whose
    /// API socket is `socket`, outlived that pane, by the window's
    /// snapshots alone. The caller has the daemon's own answer: a scanned
    /// port names its pane only when the daemon said it is closed.
    pub(crate) fn orphaned(&self, host: &Host, socket: &str, pane: Option<&str>) -> bool {
        let Some(pane) = pane.filter(|pane| !pane.is_empty()) else {
            return false;
        };
        if socket.is_empty() {
            return false;
        }
        let mut matching = self.shown.iter().filter(|shown| {
            shown.daemon.as_ref().is_some_and(|daemon| {
                daemon.host == *host && daemon.identified() && daemon.owns(socket)
            })
        });
        let Some(first) = matching.next() else {
            return false;
        };
        std::iter::once(first).chain(matching).all(|shown| {
            shown
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| !snapshot.panes.iter().any(|known| known.pane_id == pane))
        })
    }

    /// Whether process `pid` on `host`, started in `pane` of the daemon at
    /// `socket`, outlived that pane: its daemon said the pane is closed when
    /// the host was last scanned, and the snapshots agree.
    pub(crate) fn orphaned_process(&self, host: &Host, pid: u32, socket: &str, pane: &str) -> bool {
        self.closed
            .iter()
            .any(|closed| closed.pid == pid && closed.host == *host && closed.pane == pane)
            && self.orphaned(host, socket, Some(pane))
    }
}

/// Whether `snapshot` accounts for every pane: a workspace without a pane,
/// or a pane of a workspace it does not list, says it is partial.
fn complete(snapshot: &ClientShellSnapshot) -> bool {
    let workspaces: HashSet<&str> = snapshot
        .workspaces
        .iter()
        .map(|workspace| workspace.workspace_id.as_str())
        .collect();
    let with_panes: HashSet<&str> = snapshot
        .panes
        .iter()
        .map(|pane| pane.workspace_id.as_str())
        .collect();
    !workspaces.is_empty() && workspaces == with_panes
}

impl crate::SheprdWindow {
    /// The window's enabled endpoints as [`Known`] reads them.
    pub(crate) fn orphan_endpoints(&self) -> impl Iterator<Item = Endpoint<'_>> {
        self.endpoints
            .iter()
            .enumerate()
            .filter(|(_, endpoint)| endpoint.enabled)
            .map(|(index, endpoint)| {
                let live = if index == self.selected_endpoint {
                    &self.live
                } else {
                    &endpoint.live
                };
                Endpoint {
                    target: &endpoint.connection.target,
                    status: live.status,
                    snapshot: live.snapshot.as_ref(),
                    pending: live.pending_snapshot.is_some(),
                }
            })
    }

    /// The daemons of the window's enabled endpoints, the panes each has,
    /// and the processes whose panes the latest scans found closed.
    pub(crate) fn known_panes(&self) -> Known {
        let mut known = Known::default();
        for endpoint in self.orphan_endpoints() {
            known.add(endpoint);
        }
        for closed in self.listening_ports.closed() {
            known.close(closed);
        }
        known
    }
}
