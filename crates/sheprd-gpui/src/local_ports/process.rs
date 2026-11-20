//! What the OS says about the processes behind listening sockets, and
//! signalling exactly one of them.
//!
//! A pid is reused once its process exits, so every signal first looks the
//! pid up again and goes ahead only while it is proven to name the process
//! the user chose: the same start ([`Birth`]), known to be owned by this
//! user. Without that proof (no start read, an owner the OS would not
//! name, no handle where the OS has them) nothing is sent. The signal goes
//! to that process alone, never to a group or a name.
//!
//! On Linux the proof is closed by order. The handle ([`Pinned`]) is taken
//! first and the pid read after it. A read that shows the listed start
//! shows a process that began before the listing and so held the pid when
//! the handle was taken: the handle is its own. What stays open is a pid
//! reused within one clock tick of the listed start, which needs every
//! other pid to be handed out in a hundredth of a second. macOS has no
//! handle and reads whole seconds, so there a signal goes by pid right
//! after the read, as before.

use super::{
    Listener, Outcome, Started,
    birth::{self, Birth, Victim},
    naming,
    pinned::{self, Pinned},
    scan::Socket,
};
use crate::{
    listening_ports::{socket, token},
    processes::Identity,
};
use sheprd_control_protocol::protocol::hosts::PortAction;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};
use sysinfo::{
    Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, Signal, System, UpdateKind,
};

/// A process name is display text; longer ones are cut.
const NAME_LIMIT: usize = 64;
/// A command line is display text; longer ones are cut.
const COMMAND_LIMIT: usize = 512;
/// Arguments read to name a process.
const ARGUMENT_LIMIT: usize = 64;
/// How often a stopping process is looked at while it has time to exit.
const POLL: Duration = Duration::from_millis(100);

/// This process's real user, whose processes alone are listed and signalled.
pub(super) fn user() -> u32 {
    rustix::process::getuid().as_raw()
}

/// The process `pid` names now, as far as signalling cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Found {
    /// `None` when the OS would not say.
    pub birth: Option<Birth>,
    pub tie: Tie,
    /// `None` when the OS would not say.
    pub user: Option<u32>,
    /// Exited, and only waiting for its parent to reap it.
    pub zombie: bool,
}

/// How a signal can be tied to the process a look found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tie {
    // keep-sorted start
    /// Through a handle the OS keeps on the process itself, taken before
    /// the look.
    Held,
    /// This OS has such handles, and this process's could not be taken.
    Missing,
    /// This OS has no such handle (macOS): by pid, right after the look.
    Pid,
    // keep-sorted end
}

/// Whether `found` is proven to be `victim`, a running process of `user`.
/// A pid that is gone or reads another start is the victim no longer; a
/// start, an owner, or a handle that is missing proves nothing either way.
pub(crate) fn check(found: Option<Found>, victim: Victim, user: u32) -> Result<(), Outcome> {
    let found = found.ok_or(Outcome::Exited)?;
    if found.zombie {
        return Err(Outcome::Exited);
    }
    let (Some(listed), Some(read)) = (victim.birth, found.birth) else {
        return Err(Outcome::Unidentified);
    };
    if listed != read {
        return Err(Outcome::Exited);
    }
    match found.user {
        Some(owner) if owner != user => Err(Outcome::Foreign),
        Some(_) if found.tie != Tie::Missing => Ok(()),
        Some(_) | None => Err(Outcome::Unidentified),
    }
}

/// Reads `pids` afresh. A new table every time: macOS `sysinfo` keeps the
/// state it first saw for a process it already knows, so a pause or an exit
/// since would go unseen.
fn refresh(system: &mut System, pids: &[Pid]) {
    *system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(pids),
        true,
        ProcessRefreshKind::nothing()
            .with_cwd(UpdateKind::Always)
            .with_user(UpdateKind::OnlyIfNotSet),
    );
}

fn found(system: &System, pid: u32, tie: Tie) -> Option<Found> {
    let process = system.process(Pid::from_u32(pid))?;
    Some(Found {
        birth: birth::now(pid, process.start_time()),
        tie,
        user: process.user_id().map(|uid| **uid),
        zombie: matches!(
            process.status(),
            ProcessStatus::Zombie | ProcessStatus::Dead
        ),
    })
}

/// CPU and memory of the listening processes, from one table kept across
/// scans: a process's CPU is a rate between two of its samples.
#[derive(Default)]
pub(super) struct Sampler {
    system: System,
    /// The processes the previous scan sampled.
    seen: HashSet<Identity>,
}

impl Sampler {
    /// Samples `pids`; for each identity, its CPU once it was sampled
    /// before, and its resident memory.
    fn sample(&mut self, pids: &[Pid]) -> HashMap<Identity, (Option<f32>, u64)> {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(pids),
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        let previous = std::mem::take(&mut self.seen);
        let mut usage = HashMap::new();
        for pid in pids {
            let Some(process) = self.system.process(*pid) else {
                continue;
            };
            let identity = Identity {
                pid: pid.as_u32(),
                started: process.start_time(),
            };
            let cpu = previous
                .contains(&identity)
                .then(|| process.cpu_usage().max(0.));
            self.seen.insert(identity);
            usage.insert(identity, (cpu, process.memory()));
        }
        usage
    }
}

fn display(text: &str, limit: usize) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(limit)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The rows `sockets` make: each with its process's identity, names,
/// working directory and project, CPU and memory, and state, leaving out
/// sockets whose process exited since or belongs to another user.
pub(super) fn describe(
    system: &mut System,
    sampler: &mut Sampler,
    sockets: Vec<Socket>,
    user: u32,
) -> Vec<Listener> {
    let mut pids: Vec<Pid> = sockets
        .iter()
        .map(|socket| Pid::from_u32(socket.pid))
        .collect();
    pids.sort_unstable();
    pids.dedup();
    // Read before the table and again after it (Linux): a pid whose start
    // changed in between went to another process while the table was
    // read, and its row would name one process and stop the other.
    let early: HashMap<u32, Option<Birth>> = pids
        .iter()
        .map(|pid| (pid.as_u32(), birth::early(pid.as_u32())))
        .collect();
    *system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&pids),
        true,
        ProcessRefreshKind::nothing()
            .with_cwd(UpdateKind::Always)
            .with_cmd(UpdateKind::Always)
            .with_exe(UpdateKind::Always)
            .with_environ(UpdateKind::Always)
            .with_user(UpdateKind::Always),
    );
    let usage = sampler.sample(&pids);
    // Projects are looked up once per directory, not once per socket.
    let mut projects: HashMap<PathBuf, Option<String>> = HashMap::new();
    sockets
        .into_iter()
        .filter_map(|socket| {
            let process = system.process(Pid::from_u32(socket.pid))?;
            if process.user_id().is_some_and(|uid| **uid != user) {
                return None;
            }
            let identity = Identity {
                pid: socket.pid,
                started: process.start_time(),
            };
            let birth = birth::now(socket.pid, identity.started);
            if birth::BRACKETS && early.get(&socket.pid).copied().flatten() != birth {
                return None;
            }
            // `lsof` cuts names at nine characters; the OS's own is longer.
            let own = display(&process.name().to_string_lossy(), NAME_LIMIT);
            let process_name = if own.is_empty() { socket.process } else { own };
            let arguments: Vec<String> = process
                .cmd()
                .iter()
                .take(ARGUMENT_LIMIT)
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect();
            let directory = process.cwd().map(Path::to_path_buf);
            let project = directory.as_ref().and_then(|directory| {
                projects
                    .entry(directory.clone())
                    .or_insert_with(|| naming::project(directory))
                    .clone()
            });
            let (cpu, memory) = usage.get(&identity).copied().unwrap_or_default();
            Some(Listener {
                identity,
                birth,
                number: socket.number,
                bind: socket.bind,
                bound: socket.bound,
                name: naming::name(&process_name, process.exe(), &arguments),
                command: display(&arguments.join(" "), COMMAND_LIMIT),
                system: naming::system(&process_name),
                process: process_name,
                directory,
                project,
                cpu,
                memory,
                paused: process.status() == ProcessStatus::Stop,
                started: started(process.environ()),
            })
        })
        .collect()
}

/// The Herdr pane an environment names, read as the workspace chips' scan
/// reads it: the first of each variable, bounded, and only short tokens.
pub(crate) fn started(environment: &[std::ffi::OsString]) -> Option<Started> {
    let value = |name: &str| {
        environment.iter().find_map(|entry| {
            let entry = entry.to_str()?;
            entry.strip_prefix(name)?.strip_prefix('=')
        })
    };
    let workspace = value("HERDR_WORKSPACE_ID").filter(|id| token(id))?;
    let pane = value("HERDR_PANE_ID").filter(|id| token(id))?;
    Some(Started {
        pane: pane.to_owned(),
        socket: value("HERDR_SOCKET_PATH").map_or("", socket).to_owned(),
        workspace: workspace.to_owned(),
    })
}

/// Looks `pid` up afresh.
fn look(system: &mut System, pid: u32, tie: Tie) -> Option<Found> {
    refresh(system, &[Pid::from_u32(pid)]);
    found(system, pid, tie)
}

fn send(system: &System, pid: u32, signal: Signal) -> bool {
    system
        .process(Pid::from_u32(pid))
        .and_then(|process| process.kill_with(signal))
        .unwrap_or(false)
}

/// Does `action` to each of `victims` that is still that process. Blocking:
/// a worker's call.
pub(super) fn act(
    system: &mut System,
    action: PortAction,
    victims: &[Victim],
    user: u32,
    grace: Duration,
) -> Vec<(Identity, Outcome)> {
    // Held before the first look, so what the look proves of a pid is true
    // of the process every later signal goes to.
    let pinned: HashMap<Identity, Pinned> = victims
        .iter()
        .filter_map(|victim| Some((victim.identity, Pinned::open(victim.identity.pid)?)))
        .collect();
    let tie = |victim: &Identity| match pinned.get(victim) {
        Some(_) => Tie::Held,
        None if pinned::HOLDS => Tie::Missing,
        None => Tie::Pid,
    };
    let system = RefCell::new(system);
    act_with(
        action,
        victims,
        user,
        grace,
        |victim| look(&mut system.borrow_mut(), victim.pid, tie(&victim)),
        |victim, signal| match pinned.get(&victim) {
            Some(process) => process.send(signal),
            // Where the OS holds processes, none is signalled by its pid.
            None if pinned::HOLDS => false,
            None => send(&system.borrow(), victim.pid, signal),
        },
        thread::sleep,
    )
}

/// [`act`]'s steps, with the OS behind `look` (the pid's process now),
/// `send`, and `wait`. Every signal goes out only once a fresh look proves
/// the pid still names its victim ([`check`]). Stopping sends SIGTERM to them all, then
/// SIGCONT so a paused one can act on it, and SIGKILL to those still running
/// after one shared `grace`, so a batch takes no longer than one process. A
/// process that exits and whose pid is reused in between is never resumed
/// or killed.
pub(crate) fn act_with(
    action: PortAction,
    victims: &[Victim],
    user: u32,
    grace: Duration,
    mut look: impl FnMut(Identity) -> Option<Found>,
    mut send: impl FnMut(Identity, Signal) -> bool,
    mut wait: impl FnMut(Duration),
) -> Vec<(Identity, Outcome)> {
    let signal = match action {
        PortAction::Stop => Signal::Term,
        PortAction::Pause => Signal::Stop,
        PortAction::Resume => Signal::Continue,
    };
    let mut outcomes: Vec<(Identity, Outcome)> = victims
        .iter()
        .map(|&victim| {
            let outcome = match check(look(victim.identity), victim, user) {
                Err(outcome) => outcome,
                Ok(()) if send(victim.identity, signal) => Outcome::Signalled,
                Ok(()) => Outcome::Failed,
            };
            (victim.identity, outcome)
        })
        .collect();
    if action != PortAction::Stop {
        return outcomes;
    }
    let mut running: Vec<usize> = (0..outcomes.len())
        .filter(|&index| outcomes[index].1 == Outcome::Signalled)
        .collect();
    running.retain(|&index| {
        let victim = victims[index];
        let alive = check(look(victim.identity), victim, user).is_ok();
        if alive {
            send(victim.identity, Signal::Continue);
        }
        alive
    });
    let mut waited = Duration::ZERO;
    while !running.is_empty() && waited < grace {
        wait(POLL);
        waited += POLL;
        running.retain(|&index| {
            let victim = victims[index];
            check(look(victim.identity), victim, user).is_ok()
        });
    }
    for index in running {
        let victim = victims[index];
        if check(look(victim.identity), victim, user).is_ok() {
            outcomes[index].1 = if send(victim.identity, Signal::Kill) {
                Outcome::Killed
            } else {
                Outcome::Failed
            };
        }
    }
    outcomes
}
