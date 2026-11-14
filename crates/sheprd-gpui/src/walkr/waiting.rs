//! The pages that wait for the owner, counted on every counted host: the
//! Compare sessions whose status is `open`, on this computer and on each
//! SSH host the window shows. The activity bar shows the sum on Walkr's
//! icon, and a click goes to the newest one ([`target`]).
//!
//! Each host has a reader thread ([`worker`]). Reads start when the window
//! opens, every [`REFRESH`], when the section shows, and when a run
//! connects or ends. A host whose read fails keeps its last count, and the
//! section shows the failure.
//!
//! The page of a waiting session opens in the section from the address the
//! host's Walkr server gives ([`page`], [`remote`]): directly on this
//! computer, and through an SSH tunnel on an SSH host.
mod error;
mod page;
pub(crate) mod remote;
pub(crate) mod target;
mod window;
mod worker;

pub(crate) use error::Error;

use super::{history::pending::Tally, run::Parsed};
use crate::{automations::Machine, listening_ports::tunnel::Tunnel, teleport::Host};
use chrono::{DateTime, Utc};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

/// How often every host is read again.
pub(crate) const REFRESH: Duration = Duration::from_secs(60);

/// One counted host.
#[derive(Debug)]
pub(crate) struct HostWaiting {
    /// The failure of the last read, shown in the section.
    pub(crate) error: Option<Error>,
    /// Where its reads and a page's address run.
    host: Host,
    pub(crate) label: String,
    pub(crate) machine: Machine,
    reader: Option<worker::Reader>,
    /// The last read that answered.
    pub(crate) tally: Option<Tally>,
}

#[derive(Debug, Default)]
pub(crate) struct Waiting {
    /// When every host was last asked.
    asked_at: Option<Instant>,
    hosts: Vec<HostWaiting>,
    /// The page of a waiting session, picked by a click: of an SSH host,
    /// or of this computer when no live run shows one.
    pub(crate) remote: Option<RemotePage>,
    /// Numbers the remote pages, so that a late answer of an earlier one
    /// is dropped.
    serial: u64,
    /// No workspace matched the page of the last click.
    pub(crate) unmatched: bool,
    /// A read of every host is due before [`REFRESH`].
    wanted: bool,
}

/// A waiting session of a host, and its page in the section.
#[derive(Debug)]
pub(crate) struct RemotePage {
    pub(crate) id: String,
    pub(crate) machine: Machine,
    serial: u64,
    pub(crate) state: RemoteState,
}

#[derive(Debug)]
pub(crate) enum RemoteState {
    /// Its address is read, or its tunnel opens.
    Opening,
    Failed(Error),
    // A build without pages never opens one; see [`Opened`].
    #[cfg_attr(not(embedded_pages), allow(dead_code))]
    Open(Opened),
}

/// A page that the section may show: its address, and on an SSH host the
/// tunnel it goes through, which lives as long as this does. A build
/// without pages never opens one ([`Error::NoPages`]), so it reads no
/// address here.
pub(crate) struct Opened {
    page: Parsed,
    _tunnel: Option<Tunnel>,
}

// By hand: `Parsed` prints host and port only, and a tunnel has no `Debug`.
impl std::fmt::Debug for Opened {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Opened")
            .field("page", &self.page)
            .finish_non_exhaustive()
    }
}

impl Waiting {
    /// A read of every host is due: the section showed, or a run connected
    /// or ended.
    pub(crate) fn want(&mut self) {
        self.wanted = true;
    }

    /// Whether the hosts are read now: asked for, or [`REFRESH`] passed.
    pub(crate) fn due(&self, now: Instant) -> bool {
        self.wanted
            || self
                .asked_at
                .is_none_or(|at| now.saturating_duration_since(at) >= REFRESH)
    }

    /// The counted hosts become `wanted`, in its order. A host that leaves
    /// drops its reader and its count.
    pub(crate) fn set_hosts(&mut self, wanted: Vec<(Machine, String, Host)>) {
        let mut before = std::mem::take(&mut self.hosts);
        for (machine, label, host) in wanted {
            let known = before
                .iter()
                .position(|known| known.machine == machine)
                .map(|at| before.swap_remove(at));
            let mut kept = known.unwrap_or_else(|| HostWaiting {
                error: None,
                host: host.clone(),
                label: String::new(),
                machine: machine.clone(),
                reader: None,
                tally: None,
            });
            kept.label = label;
            if kept.reader.is_none() {
                match worker::start(&machine, host) {
                    Ok(reader) => kept.reader = Some(reader),
                    Err(error) => kept.error = Some(error),
                }
            }
            self.hosts.push(kept);
        }
    }

    /// Asks every host for a read.
    pub(crate) fn ask(&mut self, now: Instant) {
        for host in &self.hosts {
            if let Some(reader) = &host.reader {
                reader.ask();
            }
        }
        self.asked_at = Some(now);
        self.wanted = false;
    }

    /// Takes the answers of the readers. Whether anything shown changed.
    pub(crate) fn take_answers(&mut self) -> bool {
        let mut changed = false;
        for host in &mut self.hosts {
            let Some(reader) = &host.reader else {
                continue;
            };
            loop {
                match reader.answers.try_recv() {
                    Ok(Ok(tally)) => {
                        changed |= host.tally.as_ref() != Some(&tally) || host.error.is_some();
                        host.tally = Some(tally);
                        host.error = None;
                    }
                    Ok(Err(error)) => {
                        host.error = Some(error);
                        changed = true;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        // The thread ended; the next round starts another.
                        host.reader = None;
                        break;
                    }
                }
            }
        }
        changed
    }

    /// The counted hosts, this computer first.
    pub(crate) fn hosts(&self) -> &[HostWaiting] {
        &self.hosts
    }

    /// This computer, once its reader runs.
    pub(crate) fn local(&self) -> Option<&HostWaiting> {
        self.hosts.iter().find(|host| host.machine.is_local())
    }

    /// The pages that wait on every counted host, as each last answered.
    pub(crate) fn total(&self) -> usize {
        self.hosts
            .iter()
            .filter_map(|host| host.tally.as_ref())
            .map(|tally| tally.waiting)
            .sum()
    }

    /// The host of `machine`, for the address of a page there.
    fn host_of(&self, machine: &Machine) -> Option<Host> {
        self.hosts
            .iter()
            .find(|host| host.machine == *machine)
            .map(|host| host.host.clone())
    }

    /// The page of session `id` of `machine` starts to open. Its serial.
    fn begin_remote(&mut self, machine: Machine, id: String, state: RemoteState) -> u64 {
        self.serial = self.serial.wrapping_add(1);
        self.remote = Some(RemotePage {
            id,
            machine,
            serial: self.serial,
            state,
        });
        self.serial
    }

    /// The answer for the remote page `serial`. Whether its page opened; a
    /// late answer is dropped, and its tunnel with it.
    fn finish_remote(
        &mut self,
        serial: u64,
        result: Result<(Parsed, Option<Tunnel>), Error>,
    ) -> bool {
        let Some(remote) = self.remote.as_mut().filter(|remote| {
            remote.serial == serial && matches!(remote.state, RemoteState::Opening)
        }) else {
            return false;
        };
        match result {
            Ok((page, tunnel)) => {
                remote.state = RemoteState::Open(Opened {
                    page,
                    _tunnel: tunnel,
                });
                true
            }
            Err(error) => {
                remote.state = RemoteState::Failed(error);
                false
            }
        }
    }

    /// The remote page that is open, and its serial.
    #[cfg(embedded_pages)]
    pub(crate) fn remote_page(&self) -> Option<(&Parsed, u64)> {
        match &self.remote.as_ref()?.state {
            RemoteState::Open(opened) => Some((&opened.page, self.remote.as_ref()?.serial)),
            _ => None,
        }
    }

    /// Whether the page of session `id` of this computer is open. It then
    /// shows in place of the session's text.
    pub(crate) fn local_page_open(&self, id: &str) -> bool {
        self.remote.as_ref().is_some_and(|remote| {
            remote.machine.is_local()
                && remote.id == id
                && matches!(remote.state, RemoteState::Open(_))
        })
    }

    /// The remote page goes, and its tunnel closes.
    pub(crate) fn close_remote(&mut self) {
        self.remote = None;
    }

    /// When the newest waiting page was made.
    pub(crate) fn latest(&self) -> Option<DateTime<Utc>> {
        self.hosts
            .iter()
            .filter_map(|host| host.tally.as_ref()?.newest.first()?.created)
            .max()
    }

    /// The reads and page addresses of `machine` take `home` as its home.
    #[cfg(test)]
    pub(crate) fn test_home(&mut self, machine: &Machine, home: &std::path::Path) {
        for known in self
            .hosts
            .iter_mut()
            .filter(|known| known.machine == *machine)
        {
            known.host.env = vec![("HOME".into(), home.display().to_string())];
        }
    }

    /// A host answered `tally`, as its reader would.
    #[cfg(test)]
    pub(crate) fn test_answered(&mut self, machine: Machine, label: &str, tally: Tally) {
        let target = match &machine {
            Machine::Ssh(target) => sheprd_client::ConnectTarget::Ssh {
                target: target.clone(),
                session: String::new(),
            },
            _ => sheprd_client::ConnectTarget::Local,
        };
        let Ok(host) = Host::new(&target) else {
            return;
        };
        self.hosts.retain(|known| known.machine != machine);
        self.hosts.push(HostWaiting {
            error: None,
            host,
            label: label.to_owned(),
            machine,
            reader: None,
            tally: Some(tally),
        });
    }
}
