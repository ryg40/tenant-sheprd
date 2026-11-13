//! Which listeners outlived their panes: only one whose daemon said its pane
//! is closed and whose daemon's current, complete snapshot lacks it.
use super::*;
use crate::{
    listening_ports::{Closed, Endpoint, Known},
    state::ConnectionStatus,
};
use sheprd_client::protocol::ClientShellSnapshot;
use std::{path::PathBuf, sync::Arc};

const SOCKET: &str = "/run/user/1000/herdr/herdr.sock";
const OTHER: &str = "/run/user/1000/herdr/sessions/b/herdr.sock";

/// The endpoint of the daemon whose API socket is [`SOCKET`].
fn target() -> ConnectTarget {
    ConnectTarget::Socket(PathBuf::from("/run/user/1000/herdr/herdr-client.sock"))
}

fn daemon() -> Daemon {
    Daemon::of(&target()).unwrap()
}

/// Workspace `w0` with `panes`, and any `bare` workspaces with none. A pane
/// `wN:pM` sits in workspace `wN`, which is listed.
fn snapshot(panes: &[&str], bare: &[&str]) -> Arc<ClientShellSnapshot> {
    let mut value = serde_json::to_value(crate::sidebar::layout_tests::snapshot(1)).unwrap();
    let workspace = value["workspaces"][0].clone();
    let homes = panes
        .iter()
        .filter_map(|pane| pane.split_once(':').map(|(home, _)| home))
        .filter(|home| *home != "w0");
    for id in bare.iter().copied().chain(homes) {
        let mut extra = workspace.clone();
        extra["workspace_id"] = (*id).into();
        value["workspaces"].as_array_mut().unwrap().push(extra);
    }
    value["panes"] = panes
        .iter()
        .map(|pane| {
            serde_json::json!({
                "pane_id": pane, "workspace_id": pane.split_once(':').map_or("w0", |(home, _)| home),
                "tab_id": "t0", "label": null,
                "cwd": null, "foreground_cwd": null, "focused": false,
                "right_click_passthrough": false
            })
        })
        .collect();
    Arc::new(serde_json::from_value(value).unwrap())
}

fn known(
    status: ConnectionStatus,
    snapshot: Option<Arc<ClientShellSnapshot>>,
    pending: bool,
) -> Known {
    let mut known = Known::default();
    known.add(Endpoint {
        target: &target(),
        status,
        snapshot: snapshot.as_ref(),
        pending,
    });
    known
}

fn connected(panes: &[&str]) -> Known {
    known(
        ConnectionStatus::Connected,
        Some(snapshot(panes, &[])),
        false,
    )
}

#[test]
fn a_pane_absent_from_a_complete_snapshot_is_orphaned() {
    assert!(connected(&["w0:p1"]).orphaned(&Host::Local, SOCKET, Some("w0:p9")));
}

#[test]
fn a_live_pane_is_not_orphaned() {
    assert!(!connected(&["w0:p1", "w0:p9"]).orphaned(&Host::Local, SOCKET, Some("w0:p9")));
}

#[test]
fn another_daemons_or_hosts_listener_is_not_orphaned() {
    let known = connected(&["w0:p1"]);
    assert!(!known.orphaned(&Host::Local, OTHER, Some("w0:p9")));
    assert!(!known.orphaned(&Host::Ssh("devbox".into()), SOCKET, Some("w0:p9")));
    // A process that did not name its daemon could be any daemon's.
    assert!(!known.orphaned(&Host::Local, "", Some("w0:p9")));
}

#[test]
fn a_listener_without_a_pane_is_not_orphaned() {
    let known = connected(&["w0:p1"]);
    assert!(!known.orphaned(&Host::Local, SOCKET, None));
    assert!(!known.orphaned(&Host::Local, SOCKET, Some("")));
}

#[test]
fn a_stale_partial_or_missing_snapshot_orphans_nothing() {
    let shown = Some(snapshot(&["w0:p1"], &[]));
    for known in [
        known(ConnectionStatus::Disconnected, shown.clone(), false),
        known(ConnectionStatus::AwaitingSnapshot, shown.clone(), false),
        known(ConnectionStatus::Connecting, shown.clone(), false),
        known(ConnectionStatus::Connected, shown.clone(), true),
        known(ConnectionStatus::Connected, None, false),
        // A workspace listed without its panes: the snapshot is partial.
        known(
            ConnectionStatus::Connected,
            Some(snapshot(&["w0:p1"], &["w1"])),
            false,
        ),
        // No daemon shown at all.
        Known::default(),
    ] {
        assert!(!known.orphaned(&Host::Local, SOCKET, Some("w0:p9")));
    }
}

/// `known` with the daemon's answer that process `pid`'s pane is closed.
fn answered(mut known: Known, pid: u32, pane: &str) -> Known {
    known.close(Closed {
        host: Host::Local,
        pane: pane.into(),
        pid,
    });
    known
}

#[test]
fn a_pane_absent_from_the_snapshot_needs_a_closed_answer() {
    // `w0:p2` moved to workspace `w1` as `w1:p5`: its old ID is gone from
    // the snapshot, and its server still carries it. Herdr resolves the old
    // ID to the moved pane, so the scan has no closed answer for it.
    let known = connected(&["w0:p1", "w1:p5"]);
    assert!(!known.orphaned_process(&Host::Local, 42, SOCKET, "w0:p2"));
    // Once the daemon says the pane is closed, the same process is.
    let known = answered(known, 42, "w0:p2");
    assert!(known.orphaned_process(&Host::Local, 42, SOCKET, "w0:p2"));
}

#[test]
fn the_scan_marks_only_the_orphaned_port() {
    let scan = parse(&format!(
        "L 1 *:3000 node\nL 2 *:5173 vite\nE 1 w0 {SOCKET}\nP 1 w0:p9\nE 2 w0 {SOCKET}\n"
    ))
    .unwrap();
    let mut ports = ListeningPorts::default();
    ports.seed(Host::Local, scan);
    assert!(ports.classify(&connected(&["w0:p1"])));
    let listed = ports.get(&daemon(), "w0").unwrap();
    let orphaned: Vec<u16> = listed
        .ports
        .iter()
        .filter(|port| listed.orphaned(port))
        .map(|port| port.number)
        .collect();
    assert_eq!(orphaned, [3000]);
}
