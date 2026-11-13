//! The real scan step against this machine's tools, with a stand-in `herdr`
//! as each listener's daemon: only a pane the daemon cannot find counts as
//! closed, a process of this app (the test, the scan shell's parent) never
//! does, and a daemon the step was not given is never asked. Every listener
//! here is a `python3` this test started and stops by its exact pid, and
//! names a socket no daemon serves.
use super::super::scan::{Asked, step};
use super::*;
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

/// Listens on a free loopback port for a minute at most.
const LISTEN: &str = "import socket, time
s = socket.socket()
s.bind(('127.0.0.1', 0))
s.listen()
time.sleep(60)";

/// The daemon socket every listener here names.
const SOCKET: &str = "/nonexistent/herdr/herdr.sock";

/// A `herdr` that, like Herdr after `pane move`, resolves the moved pane's
/// old ID `w0:p2`, and finds no other pane. It notes each call beside
/// itself.
const HERDR: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$0.calls"
case "$3" in
    w0:p2) echo '{"id":"cli:pane:get","result":{"type":"pane_info","pane":{"pane_id":"w1:p5"}}}' ;;
    *) echo '{"error":{"code":"pane_not_found","message":"pane not found"},"id":"cli:pane:get"}' >&2; exit 1 ;;
esac
"#;

/// The listeners a test started, stopped however it ends.
#[derive(Default)]
struct Listeners {
    children: Vec<Child>,
    detached: Vec<u32>,
}

impl Drop for Listeners {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
        for pid in &self.detached {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
    }
}

impl Listeners {
    /// The pane variables Herdr sets.
    fn command(program: &str, pane: &str) -> Command {
        let mut command = Command::new(program);
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HERDR_PANE_ID", pane)
            .env("HERDR_SOCKET_PATH", SOCKET)
            .env("HERDR_WORKSPACE_ID", "w0")
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    /// A listener this process starts, so a descendant of the app.
    fn child(&mut self, pane: &str) -> u32 {
        let child = Self::command("python3", pane)
            .args(["-c", LISTEN])
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        self.children.push(child);
        pid
    }

    /// A listener whose parent exits at once, as a pane's shell does when the
    /// pane closes, so it is re-parented away from this process.
    fn detached(&mut self, pane: &str) -> u32 {
        let output = Self::command("/bin/sh", pane)
            .args([
                "-c",
                "python3 -c \"$0\" </dev/null >/dev/null 2>&1 & echo $!",
                LISTEN,
            ])
            .output()
            .unwrap();
        let pid = String::from_utf8(output.stdout)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        self.detached.push(pid);
        pid
    }
}

/// The stand-in `herdr`, or `None` without `python3` to listen with.
fn herdr(dir: &Path) -> Option<PathBuf> {
    let python = Command::new("python3")
        .args(["-c", "pass"])
        .status()
        .is_ok_and(|status| status.success());
    if !python {
        eprintln!("skipped: no python3 to listen with");
        return None;
    }
    let path = dir.join("herdr");
    std::fs::write(&path, HERDR).unwrap();
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    Some(path)
}

/// The step that asks only about `sockets`, through `herdr`.
fn asking(sockets: &[&str], herdr: &Path) -> String {
    let asked = Asked {
        sockets: sockets.iter().map(|socket| (*socket).to_owned()).collect(),
        sessions: Vec::new(),
    };
    step(&asked, herdr.to_str().unwrap())
}

/// The first scan by `step` that lists every one of `pids`, or `None` when
/// this host has neither `ss` nor `lsof`.
fn scan_listing(step: &str, pids: &[u32]) -> Option<Scan> {
    let mut shell = super::super::local_shell().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let output = shell.run(step, super::super::STEP_TIMEOUT).unwrap();
        if output.stdout.lines().any(|line| line == "N") {
            eprintln!("skipped: neither ss nor lsof");
            return None;
        }
        let scan = parse(&output.stdout).unwrap();
        if pids.iter().all(|pid| scan.pids.contains_key(pid)) {
            return Some(scan);
        }
        assert!(Instant::now() < deadline, "{pids:?} never listed");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn the_server_of_a_moved_pane_is_not_found_closed() {
    let dir = tempfile::tempdir().unwrap();
    let Some(herdr) = herdr(dir.path()) else {
        return;
    };
    let mut listeners = Listeners::default();
    let moved = listeners.detached("w0:p2");
    let closed = listeners.detached("w0:p9");
    let Some(scan) = scan_listing(&asking(&[SOCKET], &herdr), &[moved, closed]) else {
        return;
    };
    assert!(!scan.closed.contains_key(&moved), "{:?}", scan.closed);
    assert_eq!(scan.closed.get(&closed).map(String::as_str), Some("w0:p9"));
}

#[test]
fn a_descendant_of_the_app_is_never_orphaned() {
    let dir = tempfile::tempdir().unwrap();
    let Some(herdr) = herdr(dir.path()) else {
        return;
    };
    let mut listeners = Listeners::default();
    // Both name the same closed pane; only the app's own child is spared.
    let own = listeners.child("w0:p9");
    let other = listeners.detached("w0:p9");
    let Some(scan) = scan_listing(&asking(&[SOCKET], &herdr), &[own, other]) else {
        return;
    };
    assert!(!scan.closed.contains_key(&own), "{:?}", scan.closed);
    assert!(scan.closed.contains_key(&other), "{:?}", scan.closed);
}

#[test]
fn a_daemon_the_window_does_not_show_is_never_asked() {
    let dir = tempfile::tempdir().unwrap();
    let Some(herdr) = herdr(dir.path()) else {
        return;
    };
    let mut listeners = Listeners::default();
    let closed = listeners.detached("w0:p9");
    let step = asking(&["/nonexistent/other/herdr.sock"], &herdr);
    let Some(scan) = scan_listing(&step, &[closed]) else {
        return;
    };
    assert!(scan.closed.is_empty(), "{:?}", scan.closed);
    assert!(!dir.path().join("herdr.calls").exists());
}
