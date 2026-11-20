//! TCP ports each workspace's processes listen on, for the sidebar and the
//! status bar, where clicking one opens it in a browser tab.
//!
//! Every host has its own worker thread scanning it every few seconds through
//! one long-lived `sh`: a local one for this machine, the host's own over SSH
//! for a remote one, and a distribution's own through `wsl.exe` for WSL. The UI thread only starts and stops workers and takes
//! their answers on later ticks, so a slow `lsof` or an unreachable host never
//! blocks rendering or delays another host.
// i18n: migrated

mod error;
mod orphans;
mod render;
mod scan;
pub(crate) mod tunnel;

pub(crate) use error::Error;
pub(crate) use orphans::{Closed, Endpoint, Known};

#[cfg(test)]
mod tests;

use crate::{
    Result,
    system_load::Stop,
    usage::{Host, Shell},
};
pub(crate) use render::{chips, close_button};
pub(crate) use scan::Owner;
pub(crate) use scan::{Asked, Bind, Bound, Link, Origin, Port, Scan, parse, socket, token};
// The Ports section's Unix scan reads its addresses the same way.
#[cfg(unix)]
pub(crate) use scan::{bound, listener};
use sheprd_client::ConnectTarget;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};
#[cfg(all(test, unix))]
pub(crate) use tunnel::Tunnel;
pub(crate) use tunnel::{Key, Tunnels};

/// Ports open and close at human pace; `lsof` is not free on a busy machine.
const INTERVAL: Duration = Duration::from_secs(5);
/// A host that could not be scanned is tried again after this long.
const RETRY: Duration = Duration::from_secs(30);
const STEP_TIMEOUT: Duration = Duration::from_secs(15);
/// Hosts are few; an unbounded endpoint list still cannot start more workers.
const HOST_LIMIT: usize = 16;

/// The daemon an endpoint shows. A port is listed under one of its
/// workspaces only when the port's pane belongs to this daemon, so two
/// sessions on one machine never claim each other's servers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Daemon {
    host: Host,
    identity: Option<Identity>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Identity {
    /// This machine's daemon, by the client socket the endpoint dials.
    Client(PathBuf),
    /// A remote daemon, by its session name; its paths are the remote's own.
    Session(String),
}

impl Daemon {
    /// The daemon behind `target`, or `None` for a cloud machine, whose ports
    /// cannot be scanned without its provider's command.
    pub(crate) fn of(target: &ConnectTarget) -> Option<Self> {
        let host = Host::of(target)?;
        let identity = match target.remote_session() {
            Some(session) => Some(Identity::Session(session.to_owned())),
            None => target.socket_path().ok().map(Identity::Client),
        };
        Some(Self { host, identity })
    }

    pub(crate) fn host(&self) -> &Host {
        &self.host
    }

    /// Whether the daemon's socket or session is known, so [`Self::owns`]
    /// tells its panes from another daemon's rather than accepting all.
    pub(crate) fn identified(&self) -> bool {
        self.identity.is_some()
    }

    /// Adds this daemon to the ones its host's scan may ask about panes:
    /// the API socket beside its client socket, or its remote session.
    fn allow(&self, asked: &mut Asked) {
        match &self.identity {
            Some(Identity::Client(client)) => {
                if let Some(api) = api_socket(client).filter(|api| !asked.sockets.contains(api)) {
                    asked.sockets.push(api);
                }
            }
            Some(Identity::Session(name)) if session(name) && !asked.sessions.contains(name) => {
                asked.sessions.push(name.clone());
            }
            Some(Identity::Session(_)) | None => {}
        }
    }

    /// Whether a pane whose `HERDR_SOCKET_PATH` was `socket` is this daemon's.
    /// A pane that did not say, from an older Herdr, is given the benefit of
    /// the doubt, as is a daemon whose socket cannot be named.
    pub(crate) fn owns(&self, socket: &str) -> bool {
        let (false, Some(identity)) = (socket.is_empty(), &self.identity) else {
            return true;
        };
        let api = Path::new(socket);
        match identity {
            Identity::Client(client) => client_socket(api).as_deref() == Some(client.as_path()),
            Identity::Session(name) => session_name(api) == Some(name.as_str()),
        }
    }
}

/// The client socket a daemon serves beside its API socket, `herdr.sock`
/// beside `herdr-client.sock`, as herdr-client's discovery derives it.
fn client_socket(api: &Path) -> Option<PathBuf> {
    let stem = api.file_stem()?.to_str()?;
    Some(api.parent()?.join(format!("{stem}-client.sock")))
}

/// The API socket beside `client`, `herdr.sock` beside `herdr-client.sock`,
/// as one line of plain text; `None` for a socket not named that way.
fn api_socket(client: &Path) -> Option<String> {
    let stem = client.file_name()?.to_str()?.strip_suffix("-client.sock")?;
    let api = client
        .parent()?
        .join(format!("{stem}.sock"))
        .into_os_string()
        .into_string()
        .ok()?;
    (!stem.is_empty() && socket(&api) == api).then_some(api)
}

/// Whether `name` is a session name the scan may put into a path: the
/// characters Herdr allows in one, and never `.` or `..`.
fn session(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !matches!(name, "." | "..")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// The `herdr` a host's scan asks about panes: the one this app starts its
/// daemon with on this computer, the host's own on its `PATH` elsewhere;
/// never one a listener names. Empty, so nothing is asked, when the path is
/// not text.
fn program(host: &Host) -> String {
    match host {
        Host::Local => crate::daemon::executable()
            .into_os_string()
            .into_string()
            .unwrap_or_default(),
        _ => "herdr".into(),
    }
}

/// `…/sessions/<name>/herdr.sock` belongs to session `name`; the socket in
/// the configuration root belongs to `default`.
fn session_name(api: &Path) -> Option<&str> {
    let directory = api.parent()?;
    if directory.parent()?.file_name()? == "sessions" {
        directory.file_name()?.to_str()
    } else {
        Some("default")
    }
}

/// The shell a host is scanned through.
fn open(host: &Host) -> Result<Shell> {
    match host {
        Host::Local => local_shell(),
        Host::Ssh(target) => Shell::connect(target).map_err(crate::Error::from),
        // A distribution is scanned as a Linux host, with its own `ss`.
        Host::Wsl(distro) => Shell::connect_wsl(distro).map_err(crate::Error::from),
        #[cfg(feature = "azure")]
        Host::Azure { .. } => Err(crate::Error::CloudHostUnsupported),
    }
    .map_err(failed)
}

#[cfg(unix)]
fn local_shell() -> Result<Shell> {
    let mut command = sheprd_client::new_command("/bin/sh");
    command
        .arg("-s")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    Ok(Shell::start(command)?)
}

/// Windows has neither `ss` nor `lsof`, and its own tools are not wired up.
#[cfg(windows)]
fn local_shell() -> Result<Shell> {
    Err(Error::Unsupported.into())
}

fn scan(shell: &mut Shell, step: &str) -> Result<Scan> {
    let output = shell.run(step, STEP_TIMEOUT).map_err(failed)?;
    parse(&output.stdout)
}

fn failed(error: impl Into<crate::Error>) -> crate::Error {
    Error::Scan(Box::new(error.into())).into()
}

struct Worker {
    stop: Arc<Stop>,
    results: mpsc::Receiver<Result<Scan>>,
}

impl Drop for Worker {
    /// The thread ends after its current scan; nothing waits for it here.
    fn drop(&mut self) {
        self.stop.stop();
    }
}

fn spawn(host: Host, asked: Arc<Mutex<Asked>>) -> Option<Worker> {
    let stop = Arc::new(Stop::default());
    let (sender, results) = mpsc::sync_channel(2);
    let shared = stop.clone();
    let spawned = thread::Builder::new()
        .name("herdr-listening-ports".into())
        .spawn(move || run(&host, &asked, &shared, &sender));
    match spawned {
        Ok(_) => Some(Worker { stop, results }),
        Err(error) => {
            tracing::warn!(category = "listening-ports", %error, "could not start a port scanner");
            None
        }
    }
}

fn run(host: &Host, asked: &Mutex<Asked>, stop: &Stop, results: &mpsc::SyncSender<Result<Scan>>) {
    let herdr = program(host);
    let mut shell: Option<Shell> = None;
    loop {
        // A poisoned list asks nothing.
        let step = asked
            .lock()
            .map(|asked| scan::step(&asked, &herdr))
            .unwrap_or_else(|_| scan::step(&Asked::default(), ""));
        let opened = match shell.take() {
            Some(shell) => Ok(shell),
            None => open(host),
        };
        let result = opened.and_then(|mut opened| {
            let ports = scan(&mut opened, &step);
            // A failed step leaves the shell unusable; a missing tool does not.
            if !matches!(ports, Err(crate::Error::ListeningPorts(Error::Scan(_)))) {
                shell = Some(opened);
            }
            ports
        });
        let due = Instant::now() + if result.is_ok() { INTERVAL } else { RETRY };
        match results.try_send(result) {
            // The UI is behind; it will take the next one.
            Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
            Err(mpsc::TrySendError::Disconnected(_)) => return,
        }
        if !stop.wait(due) {
            return;
        }
    }
}

/// One host's latest scan.
#[derive(Default)]
struct Reading {
    scan: Scan,
    /// Where the host's ports open, which follows from the host alone.
    origin: Origin,
    /// Why the latest scan failed, as last logged; the last good ports stay
    /// shown.
    error: Option<String>,
    /// The ports of each owner whose servers outlived their panes, as the
    /// last [`ListeningPorts::classify`] found them.
    orphaned: HashMap<Owner, HashSet<u16>>,
}

impl Reading {
    /// Whether anything shown changed.
    fn apply(&mut self, result: Result<Scan>) -> bool {
        match result {
            Ok(scan) => {
                self.error = None;
                if scan == self.scan {
                    return false;
                }
                self.scan = scan;
                true
            }
            Err(error) => {
                // The display boundary: the cause says what went wrong.
                let text = crate::i18n::errors::with_causes(&error);
                // Nothing shown changes; the reason is logged once, not every retry.
                if self.error.as_deref() != Some(text.as_str()) {
                    tracing::warn!(category = "listening-ports", error = ?error, "could not scan listening ports");
                    self.error = Some(text);
                }
                false
            }
        }
    }
}

#[derive(Default)]
struct Monitor {
    /// The daemons its scan may ask about panes, which its worker reads
    /// before each scan.
    asked: Arc<Mutex<Asked>>,
    /// None once its thread could not start or has ended.
    worker: Option<Worker>,
    reading: Reading,
}

/// One workspace's ports and where a browser finds them.
#[derive(Clone, Copy)]
pub(crate) struct Listed<'a> {
    pub ports: &'a [Port],
    pub origin: &'a Origin,
    /// The numbers among `ports` whose servers outlived their panes.
    pub orphaned: Option<&'a HashSet<u16>>,
}

impl Listed<'_> {
    /// Whether `port`'s server outlived the pane that started it.
    pub(crate) fn orphaned(&self, port: &Port) -> bool {
        self.orphaned
            .is_some_and(|orphaned| orphaned.contains(&port.number))
    }
}

/// Every scanned host's listening ports, keyed by host.
#[derive(Default)]
pub(crate) struct ListeningPorts {
    monitors: HashMap<Host, Monitor>,
    /// The panes the last classification went by.
    known: Known,
    /// A scan changed since the last classification.
    unclassified: bool,
}

impl ListeningPorts {
    /// The ports `workspace` of `daemon` listens on, lowest first, and where
    /// they open; None while it listens on none.
    pub fn get(&self, daemon: &Daemon, workspace: &str) -> Option<Listed<'_>> {
        let reading = &self.monitors.get(&daemon.host)?.reading;
        reading.scan.ports.iter().find_map(|(owner, ports)| {
            (owner.workspace == workspace && daemon.owns(&owner.socket) && !ports.is_empty())
                .then_some(Listed {
                    ports,
                    origin: &reading.origin,
                    orphaned: reading.orphaned.get(owner),
                })
        })
    }

    /// The workspace of `daemon` whose pane started the listening process
    /// `pid`, as the latest scan of its host found it.
    pub fn workspace_of(&self, daemon: &Daemon, pid: u32) -> Option<&str> {
        let owner = self
            .monitors
            .get(&daemon.host)?
            .reading
            .scan
            .pids
            .get(&pid)?;
        daemon
            .owns(&owner.socket)
            .then_some(owner.workspace.as_str())
    }

    /// Whether a port listed on `key`'s host still tunnels to exactly `key`.
    /// A server that moved to another address on the same number no longer
    /// does, so its old tunnel goes rather than lingering without a close
    /// button beside the new one.
    pub fn tunnels_to(&self, key: &Key) -> bool {
        let Some(monitor) = self.monitors.get(&Host::Ssh(key.target.clone())) else {
            return false;
        };
        let reading = &monitor.reading;
        reading
            .scan
            .ports
            .values()
            .flatten()
            .any(|port| matches!(port.link(&reading.origin), Some(Link::Tunnel(listed)) if listed == *key))
    }

    /// Every scanned process whose daemon said its pane is closed.
    pub fn closed(&self) -> impl Iterator<Item = Closed> + '_ {
        self.monitors.iter().flat_map(|(host, monitor)| {
            monitor
                .reading
                .scan
                .closed
                .iter()
                .map(|(pid, pane)| Closed {
                    host: host.clone(),
                    pane: pane.clone(),
                    pid: *pid,
                })
        })
    }

    /// Whether the marks are still those of `endpoints` and every scan
    /// taken, so classifying again would change nothing. It allocates
    /// nothing, as the window asks every frame.
    pub fn classified<'a>(&self, endpoints: impl IntoIterator<Item = Endpoint<'a>>) -> bool {
        !self.unclassified && self.known.same(endpoints)
    }

    /// Marks the ports whose servers outlived their panes, by `known`, when
    /// it or a scan changed since the last time. Whether any mark changed.
    pub fn classify(&mut self, known: &Known) -> bool {
        if !self.unclassified && self.known == *known {
            return false;
        }
        self.unclassified = false;
        self.known = known.clone();
        let mut changed = false;
        for (host, monitor) in &mut self.monitors {
            let reading = &mut monitor.reading;
            let orphaned: HashMap<Owner, HashSet<u16>> = reading
                .scan
                .ports
                .iter()
                .filter_map(|(owner, ports)| {
                    let numbers: HashSet<u16> = ports
                        .iter()
                        .filter(|port| known.orphaned(host, &owner.socket, port.pane.as_deref()))
                        .map(|port| port.number)
                        .collect();
                    (!numbers.is_empty()).then(|| (owner.clone(), numbers))
                })
                .collect();
            changed |= orphaned != reading.orphaned;
            reading.orphaned = orphaned;
        }
        changed
    }

    /// Lets each host's scan ask about panes only the daemons among
    /// `daemons` on that host, the ones the window shows; a host with none
    /// asks nothing.
    pub fn ask(&mut self, daemons: impl IntoIterator<Item = Daemon>) {
        let mut wanted: HashMap<Host, Asked> = HashMap::new();
        for daemon in daemons {
            daemon.allow(wanted.entry(daemon.host.clone()).or_default());
        }
        for (host, monitor) in &self.monitors {
            let asked = wanted.remove(host).unwrap_or_default();
            // A poisoned list stays as it was; its worker asks nothing.
            if let Ok(mut shared) = monitor.asked.lock()
                && *shared != asked
            {
                *shared = asked;
            }
        }
    }

    /// Shows `scan` for `host` as if a scan had found it, with no worker.
    #[cfg(test)]
    pub(crate) fn seed(&mut self, host: Host, scan: Scan) {
        self.unclassified = true;
        let reading = Reading {
            origin: Origin::new(&host),
            scan,
            error: None,
            orphaned: HashMap::new(),
        };
        self.monitors.insert(
            host,
            Monitor {
                worker: None,
                reading,
                ..Monitor::default()
            },
        );
    }

    /// Scans exactly `hosts` (at most [`HOST_LIMIT`]): starts workers for new
    /// ones, stops and forgets the rest, and takes finished scans. Returns
    /// whether anything shown changed.
    pub fn poll(&mut self, hosts: impl IntoIterator<Item = Host>) -> bool {
        let mut wanted: Vec<Host> = Vec::new();
        for host in hosts {
            if wanted.len() == HOST_LIMIT {
                break;
            }
            // Several sessions on one machine share its scan.
            if !wanted.contains(&host) {
                wanted.push(host);
            }
        }
        let mut changed = false;
        self.monitors.retain(|host, monitor| {
            let keep = wanted.contains(host);
            changed |= !keep && !monitor.reading.scan.ports.is_empty();
            keep
        });
        for host in wanted {
            self.monitors.entry(host).or_insert_with_key(|host| {
                let asked = Arc::default();
                Monitor {
                    worker: spawn(host.clone(), Arc::clone(&asked)),
                    asked,
                    reading: Reading {
                        origin: Origin::new(host),
                        ..Reading::default()
                    },
                }
            });
        }
        for monitor in self.monitors.values_mut() {
            while let Some(worker) = &monitor.worker {
                match worker.results.try_recv() {
                    Ok(result) => {
                        let applied = monitor.reading.apply(result);
                        self.unclassified |= applied;
                        changed |= applied;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => monitor.worker = None,
                }
            }
        }
        changed
    }
}
