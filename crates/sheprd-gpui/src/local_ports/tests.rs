#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

mod badge;
#[cfg(unix)]
mod batch;
#[cfg(target_os = "linux")]
mod birth;
mod en_xa;
#[cfg(unix)]
mod identity;
#[cfg(unix)]
mod naming;
#[cfg(target_os = "linux")]
mod pinned;
#[cfg(unix)]
mod process;
#[cfg(unix)]
mod retry;
#[cfg(unix)]
mod scan;
#[cfg(unix)]
mod scan_failures;
mod section;
mod sort;
mod state;

/// What a stop of pid `pid`, started at `started`, is asked with: the
/// fixture rows carry no finer start.
pub(super) fn victim(pid: u32, started: u64) -> Victim {
    listener(1, pid, started).victim()
}

/// A listener of pid `pid` on `number`, started at `started`.
pub(super) fn listener(number: u16, pid: u32, started: u64) -> Listener {
    Listener {
        identity: Identity { pid, started },
        birth: None,
        number,
        bind: Bind::Loopback,
        bound: vec![Bound::Address(std::net::Ipv4Addr::LOCALHOST.into())],
        process: "node".into(),
        name: "vite".into(),
        command: "node /home/dev/app/node_modules/.bin/vite".into(),
        directory: Some(PathBuf::from("/home/dev/app")),
        project: Some("app".into()),
        cpu: None,
        memory: 0,
        system: false,
        paused: false,
        started: None,
    }
}
