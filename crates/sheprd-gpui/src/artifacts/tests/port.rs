//! A click takes the server's port back: a local server that does not
//! answer gets `off` before its start, a forward that a window of the app
//! holds stops without a question, and any other program on the port is
//! only asked about, also one whose command line looks like the app's
//! forward. The port listing is the fake worker of `LocalPorts::fake`, so
//! no real process is ever signalled. Unix only, as the fake program is a
//! shell script.
use super::*;
use crate::{
    listening_ports::{Bind, Bound},
    local_ports::{Command, Listener, LocalPorts, Update, Victim},
    menu::Page,
    processes::Identity,
};
use gpui::{Modifiers, VisualTestContext};
use sheprd_control_protocol::protocol::hosts::PortAction;
use std::{net::Ipv4Addr, net::TcpListener, sync::mpsc, time::Duration, time::Instant};

fn status(state: &str, page: &WebUrl) -> String {
    format!(r#"{{"state": "{state}", "url": "{}"}}"#, page.as_str())
}

fn page_at(port: u16) -> WebUrl {
    url(&format!("http://127.0.0.1:{port}/browse"))
}

const HOLDER: Identity = Identity {
    pid: 4242,
    started: 10,
};

/// The row the port listing shows for what holds `port`.
fn holder(port: u16, process: &str, command: &str) -> Listener {
    Listener {
        identity: HOLDER,
        birth: None,
        number: port,
        bind: Bind::Loopback,
        bound: vec![Bound::Address(Ipv4Addr::LOCALHOST.into())],
        process: process.into(),
        name: process.into(),
        command: command.into(),
        directory: None,
        project: None,
        cpu: None,
        memory: 0,
        system: false,
        paused: false,
        started: None,
    }
}

/// The window takes `update` from the worker, as its tick does, and the
/// click's wait looks again.
fn deliver(
    update: Update,
    updates: &mpsc::SyncSender<Update>,
    view: &gpui::Entity<crate::SheprdWindow>,
    cx: &mut VisualTestContext,
) {
    updates.send(update).unwrap();
    view.update(cx, |view, _| view.local_ports.poll(false, Instant::now()));
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
}

fn signals(commands: &mpsc::Receiver<Command>) -> Vec<Command> {
    commands
        .try_iter()
        .filter(|command| matches!(command, Command::Signal(..)))
        .collect()
}

fn runs(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("runs"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[gpui::test]
fn a_server_that_does_not_answer_is_stopped_then_started(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let page = page_at(free_port());
    let program = fake_program(dir.path(), &status("unreachable", &page));
    let (view, cx, button) = clicking(cx, program, LocalPorts::default());
    cx.simulate_click(button, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(runs(dir.path()), ["off", "start"]);
    view.read_with(cx, |view, _| assert!(view.menu.page.is_none()));
    assert!(shows(&view, cx, &page));
}

#[gpui::test]
fn another_program_on_the_port_is_only_asked_about(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    // Stands in for the other program: the port is taken for real.
    let held = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = held.local_addr().unwrap().port();
    let program = fake_program(dir.path(), &status("stopped", &page_at(port)));
    let (ports, updates, commands) = LocalPorts::fake();
    let (view, cx, button) = clicking(cx, program, ports);
    cx.simulate_click(button, Modifiers::default());
    cx.run_until_parked();
    let listing = vec![holder(port, "python3", "python3 -m http.server")];
    deliver(
        Update::Listing(Instant::now(), Ok(listing)),
        &updates,
        &view,
        cx,
    );
    view.read_with(cx, |view, _| {
        assert_eq!(view.menu.page, Some(Page::PortSignal));
    });
    assert!(signals(&commands).is_empty());
    assert!(runs(dir.path()).is_empty());
    drop(held);
}

#[gpui::test]
fn a_forward_of_this_app_on_the_port_stops_without_a_question(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let port = free_port();
    let page = page_at(port);
    let program = fake_program(dir.path(), &status("stopped", &page));
    let (view, cx, button) = clicking(cx, program, LocalPorts::default());
    view.update(cx, |view, _| {
        view.port_forwards.fixture(
            "devbox",
            6790,
            crate::port_forward::State::Listening { local_port: port },
        );
    });
    cx.simulate_click(button, Modifiers::default());
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.menu.page.is_none());
        assert!(view.port_forwards.for_host("devbox").next().is_none());
    });
    assert_eq!(runs(dir.path()), ["start"]);
    assert!(shows(&view, cx, &page));
}

#[gpui::test]
fn an_ssh_that_looks_like_a_forward_of_this_app_is_only_asked_about(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    // Stands in for the `ssh`: the port is taken for real.
    let held = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = held.local_addr().unwrap().port();
    let program = fake_program(dir.path(), &status("stopped", &page_at(port)));
    let (ports, updates, commands) = LocalPorts::fake();
    let (view, cx, button) = clicking(cx, program, ports);
    cx.simulate_click(button, Modifiers::default());
    cx.run_until_parked();
    // The marks of this app's forward master; no window holds it.
    let command = "ssh -N -o ControlMaster=yes -o ControlPersist=no \
                   -o ControlPath=/tmp/herdr-fwd-99999-1/c -- devbox";
    let listing = vec![holder(port, "ssh", command)];
    deliver(
        Update::Listing(Instant::now(), Ok(listing)),
        &updates,
        &view,
        cx,
    );
    view.read_with(cx, |view, _| {
        assert_eq!(view.menu.page, Some(Page::PortSignal));
    });
    assert!(signals(&commands).is_empty());
    assert!(runs(dir.path()).is_empty());
    drop(held);
}

#[gpui::test]
fn a_listener_on_another_address_is_not_offered_for_a_stop(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    // Something the list does not show holds the port on IPv4 loopback.
    let held = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = held.local_addr().unwrap().port();
    let program = fake_program(dir.path(), &status("stopped", &page_at(port)));
    let (ports, updates, commands) = LocalPorts::fake();
    let (view, cx, button) = clicking(cx, program, ports);
    cx.simulate_click(button, Modifiers::default());
    cx.run_until_parked();
    // A process with the same port number on another interface, which
    // cannot hold loopback's.
    let mut elsewhere = holder(port, "python3", "python3 -m http.server");
    let address = "192.0.2.7".parse().unwrap();
    elsewhere.bind = Bind::Address(address);
    elsewhere.bound = vec![Bound::Address(address)];
    deliver(
        Update::Listing(Instant::now(), Ok(vec![elsewhere])),
        &updates,
        &view,
        cx,
    );
    view.read_with(cx, |view, _| assert!(view.menu.page.is_none()));
    assert!(signals(&commands).is_empty());
    assert!(runs(dir.path()).is_empty());
    drop(held);
}

#[gpui::test]
fn a_stop_whose_worker_ends_starts_nothing(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let held = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = held.local_addr().unwrap().port();
    let program = fake_program(dir.path(), &status("stopped", &page_at(port)));
    let (ports, updates, commands) = LocalPorts::fake();
    let (view, cx, button) = clicking(cx, program, ports);
    cx.simulate_click(button, Modifiers::default());
    cx.run_until_parked();
    let listing = vec![holder(port, "python3", "python3 -m http.server")];
    deliver(
        Update::Listing(Instant::now(), Ok(listing)),
        &updates,
        &view,
        cx,
    );
    cx.update(|window, cx| {
        window.refresh();
        crate::sidebar::layout_tests::full_draw(window, cx).clear(cx);
    });
    let yes = cx.debug_bounds("port-signal-confirm").unwrap().center();
    cx.simulate_click(yes, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        signals(&commands),
        [Command::Signal(
            PortAction::Stop,
            vec![Victim {
                identity: HOLDER,
                birth: None
            }]
        )]
    );

    // The listener lets go of the port, but the Ports worker ends before
    // its stop reports back.
    drop(held);
    drop(updates);
    view.update(cx, |view, _| view.local_ports.poll(false, Instant::now()));
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    assert!(runs(dir.path()).is_empty());
    view.read_with(cx, |view, _| assert!(view.menu.page.is_none()));
}
