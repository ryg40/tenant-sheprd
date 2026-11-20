//! The step a host's shell runs to list TCP listeners, and the text it prints.
//!
//! Herdr exports `HERDR_WORKSPACE_ID` into every pane it starts, and a server
//! launched from a pane inherits it, so a listener's own environment names
//! its workspace exactly; no process tree or working directory is guessed.
//! Listeners whose process lacks the variable (system services, containers,
//! anything started outside Herdr) belong to no workspace and are dropped.

use super::Error;
use super::tunnel::Key;
use crate::{Result, browser::WebUrl, usage::Host};
use sheprd_client::shell_quote;
use std::{collections::HashMap, net::IpAddr};

/// Listener lines read from one answer; the rest are ignored.
const MAX_LISTENERS: usize = 1024;
/// Ports kept per workspace, lowest first.
pub(crate) const MAX_PORTS: usize = 32;
/// Process names are display text only.
const MAX_PROCESS: usize = 32;
/// Workspace ids are short daemon tokens such as `w7V`.
const MAX_WORKSPACE_ID: usize = 64;
/// Longer socket paths than any Unix socket can have are not trusted.
const MAX_SOCKET: usize = 1024;

/// Prints `L <pid> <address:port> <process>` per listening socket, then
/// `E <pid> <workspace> <api socket>` for each listener started inside a
/// Herdr pane, the socket naming which daemon's pane that was, and
/// `P <pid> <pane>` when that daemon answers `pane_not_found` for the pane
/// the process names, or `N` when the host has neither `ss` nor `lsof`.
///
/// Only the daemons [`step`] names are asked (`herdr_sockets`, this
/// computer's API sockets; `herdr_sessions`, a remote host's session names),
/// through the `herdr` it names (`herdr_cli`); run alone, the step asks
/// nothing. A listener's own `HERDR_BIN_PATH` is never run. Herdr gives a
/// pane moved to another workspace a new ID and keeps resolving the old one,
/// so each pane is asked twice, once without pane context and once as the
/// pane itself (`HERDR_PANE_ID`, no other variable of the listener), and is
/// closed only when both answers exit 1 with a `pane_not_found` error. Each
/// ask is killed after 2 s by `timeout`, else by `perl`'s `alarm` (macOS);
/// with neither, nothing is asked. Asking stops after 6 s in all, and a
/// socket that timed out once is not asked again in the scan, so the step
/// ends well inside its own timeout. Nor is `P` printed for a listener that
/// is the shell's parent (the app, on this computer) or descends from it:
/// the app's own tunnels and helpers inherit the pane variables of the pane
/// it was started from without being that pane's servers. Linux prefers
/// `ss`, which is quicker; macOS has only `lsof`. Both list only this user's
/// processes with their owners, so another user's servers never show. The
/// step runs in a long-lived shell, so it must not `exit`, and it adds each
/// `PATH` entry only when missing: appending on every scan grows `PATH`
/// without bound and slows every command lookup (issue #754).
// i18n: data
pub(crate) const COMMAND: &str = r#"herdr_parent() {
    if [ -r "/proc/$1/status" ]; then
        sed -n 's/^PPid:[[:space:]]*//p' "/proc/$1/status"
    else
        ps -o ppid= -p "$1" 2>/dev/null | tr -d ' '
    fi
}
herdr_ours() {
    herdr_up=$1
    herdr_hops=0
    while [ "$herdr_hops" -lt 64 ] && [ -n "$herdr_up" ] && [ "$herdr_up" -gt 1 ] 2>/dev/null; do
        [ "$herdr_up" = "$PPID" ] && return 0
        herdr_up=$(herdr_parent "$herdr_up")
        herdr_hops=$((herdr_hops + 1))
    done
    return 1
}
herdr_known() {
    [ -n "$1" ] || return 1
    case "
$herdr_sockets
" in
        *"
$1
"*) return 0 ;;
    esac
    herdr_config=${XDG_CONFIG_HOME:-$HOME/.config}/herdr
    for herdr_name in $herdr_sessions; do
        case "$herdr_name" in
            default) herdr_expected=$herdr_config/herdr.sock ;;
            *) herdr_expected=$herdr_config/sessions/$herdr_name/herdr.sock ;;
        esac
        [ "$1" = "$herdr_expected" ] && return 0
    done
    return 1
}
herdr_bounded() {
    case "$herdr_wait" in
        timeout) timeout -s KILL 2 "$@" ;;
        perl) perl -e 'alarm shift; exec @ARGV or exit 127' 2 "$@" ;;
        *) return 127 ;;
    esac
}
herdr_ask() {
    case "$herdr_slow" in
        *"
$1
"*) return 1 ;;
    esac
    [ $(($(date +%s) - herdr_start)) -lt 6 ] || return 1
    if [ -n "$3" ]; then
        herdr_answer=$(herdr_bounded env -u HERDR_TAB_ID -u HERDR_WORKSPACE_ID HERDR_PANE_ID="$3" HERDR_SOCKET_PATH="$1" "$herdr_cli" pane get "$2" 2>&1 >/dev/null </dev/null)
    else
        herdr_answer=$(herdr_bounded env -u HERDR_PANE_ID -u HERDR_TAB_ID -u HERDR_WORKSPACE_ID HERDR_SOCKET_PATH="$1" "$herdr_cli" pane get "$2" 2>&1 >/dev/null </dev/null)
    fi
    herdr_status=$?
    if [ "$herdr_status" -eq 124 ] || [ "$herdr_status" -ge 128 ]; then
        herdr_slow="$herdr_slow$1
"
        return 1
    fi
    [ "$herdr_status" -eq 1 ] || return 1
    case "$herdr_answer" in
        '{"error":{"code":"pane_not_found",'*) return 0 ;;
    esac
    return 1
}
herdr_closed() {
    herdr_ask "$1" "$2" '' && herdr_ask "$1" "$2" "$2"
}
for herdr_dir in /usr/sbin /sbin; do
    case ":$PATH:" in
        *":$herdr_dir:"*) ;;
        *) PATH="$PATH:$herdr_dir" ;;
    esac
done
if command -v ss >/dev/null 2>&1; then
    herdr_ports=$(ss -ltnp 2>/dev/null | awk '{
        rest = $0
        while (match(rest, /\("[^"]*",pid=[0-9]+/)) {
            entry = substr(rest, RSTART + 2, RLENGTH - 2)
            rest = substr(rest, RSTART + RLENGTH)
            split(entry, part, "\",pid=")
            print "L", part[2], $4, part[1]
        }
    }')
elif command -v lsof >/dev/null 2>&1; then
    herdr_ports=$(lsof -nP -iTCP -sTCP:LISTEN -Fpcn 2>/dev/null | awk '
        /^p/ { pid = substr($0, 2) }
        /^c/ { name = substr($0, 2) }
        /^n/ { print "L", pid, substr($0, 2), name }')
else
    herdr_ports=N
fi
printf '%s\n' "$herdr_ports"
herdr_wait=
if [ -z "${herdr_cli:-}" ]; then
    :
elif command -v timeout >/dev/null 2>&1; then
    herdr_wait=timeout
elif command -v perl >/dev/null 2>&1; then
    herdr_wait=perl
fi
herdr_asked=0
herdr_start=$(date +%s)
herdr_slow='
'
herdr_gone='
'
herdr_live='
'
for herdr_pid in $(printf '%s\n' "$herdr_ports" | awk '$1 == "L" { print $2 }' | sort -un | head -n 128); do
    if [ -r "/proc/$herdr_pid/environ" ]; then
        herdr_env=$(tr '\0' '\n' < "/proc/$herdr_pid/environ")
    else
        herdr_env=$(ps -E -ww -o command= -p "$herdr_pid" 2>/dev/null | tr ' ' '\n')
    fi
    herdr_workspace=$(printf '%s\n' "$herdr_env" | sed -n 's/^HERDR_WORKSPACE_ID=//p' | head -n 1)
    herdr_pane=$(printf '%s\n' "$herdr_env" | sed -n 's/^HERDR_PANE_ID=//p' | head -n 1)
    herdr_socket=$(printf '%s\n' "$herdr_env" | sed -n 's/^HERDR_SOCKET_PATH=//p' | head -n 1)
    if [ -n "$herdr_workspace" ]; then
        printf 'E %s %s %s\n' "$herdr_pid" "$herdr_workspace" "$herdr_socket"
        case "$herdr_pane" in
            ''|-*) continue ;;
        esac
        [ -n "$herdr_wait" ] || continue
        herdr_known "$herdr_socket" || continue
        herdr_key="$herdr_socket $herdr_pane"
        case "$herdr_live" in
            *"
$herdr_key
"*) continue ;;
        esac
        case "$herdr_gone" in
            *"
$herdr_key
"*) ;;
            *)
                [ "$herdr_asked" -lt 32 ] || continue
                herdr_asked=$((herdr_asked + 1))
                if herdr_closed "$herdr_socket" "$herdr_pane"; then
                    herdr_gone="$herdr_gone
$herdr_key
"
                else
                    herdr_live="$herdr_live
$herdr_key
"
                    continue
                fi
                ;;
        esac
        herdr_ours "$herdr_pid" || printf 'P %s %s\n' "$herdr_pid" "$herdr_pane"
    fi
done
true"#;

/// The daemons a host's scan may ask about panes: only those the window
/// shows on that host, so no other daemon of the user is ever asked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Asked {
    /// API sockets of this computer's daemons.
    pub sockets: Vec<String>,
    /// Session names of a remote host's daemons, whose paths are the host's
    /// own.
    pub sessions: Vec<String>,
}

/// [`COMMAND`], asking only the daemons in `asked` through `herdr`, the
/// program this app chose; an empty `herdr` asks nothing. Each entry is one
/// quoted line, so a path is compared, never run or expanded.
pub(crate) fn step(asked: &Asked, herdr: &str) -> String {
    let lines = |items: &[String]| shell_quote(&items.join("\n"));
    format!(
        "herdr_cli={}\nherdr_sockets={}\nherdr_sessions={}\n{COMMAND}",
        shell_quote(herdr),
        lines(&asked.sockets),
        lines(&asked.sessions)
    )
}

/// The address a socket listens on, which decides how it can be reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bind {
    /// One specific, non-loopback address.
    Address(IpAddr),
    /// The host's own loopback only.
    Loopback,
    /// Every address, `*`, `0.0.0.0`, or `::`.
    Any,
}

/// The address a socket is bound to, as the listing printed it: what
/// [`Bind`] classes, kept for the question of which socket holds one
/// endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bound {
    // keep-sorted start
    /// One address, which may be a family's wildcard, `0.0.0.0` or `::`.
    Address(IpAddr),
    /// `*`, every address. `ss` prints it for an IPv6 socket that takes
    /// IPv4 connections too, and `[::]` for one that does not; `lsof`
    /// prints it for the wildcard of either family, without saying which.
    Every,
    // keep-sorted end
}

impl Bound {
    fn parse(host: &str) -> Option<Self> {
        // `ss` names the device a socket is bound to, or the interface of
        // a link-scoped address, after the address and so after its
        // brackets: `127.0.0.1%lo`, `*%lo`, `[::ffff:127.0.0.1]%lo`,
        // `[fe80::1]%eth0`. A zone can also stand inside them:
        // `[fe80::1%eth0]`. Neither changes which address it is, nor
        // whether another socket can be bound to it. A name that is there
        // must be a name: `[::1]%` is no address.
        let named = |name: &str| !name.is_empty();
        let host = match host.strip_prefix('[') {
            Some(rest) => {
                let (inner, after) = rest.split_once(']')?;
                if !(after.is_empty() || after.strip_prefix('%').is_some_and(named)) {
                    return None;
                }
                inner
            }
            None => host,
        };
        let host = match host.split_once('%') {
            Some((address, name)) if named(name) => address,
            Some(_) => return None,
            None => host,
        };
        if host == "*" {
            return Some(Self::Every);
        }
        host.parse().ok().map(Self::Address)
    }

    /// Whether a socket bound here holds `endpoint`, so that it would make
    /// a bind of `endpoint` fail: the same address, the wildcard of its
    /// family, or `*`. Another address of the loopback network does not,
    /// nor does IPv6's loopback or its wildcard hold IPv4's. An IPv4
    /// address written as IPv6 (`::ffff:127.0.0.1`) counts as that IPv4
    /// address. The device a socket is bound to does not count: a socket
    /// bound to another device (`0.0.0.0%eth0`) takes no connection to the
    /// loopback, but the bind fails beside it all the same.
    pub(crate) fn holds(self, endpoint: IpAddr) -> bool {
        let Self::Address(address) = self else {
            return true;
        };
        let (address, endpoint) = (address.to_canonical(), endpoint.to_canonical());
        address == endpoint || (address.is_unspecified() && address.is_ipv4() == endpoint.is_ipv4())
    }
}

impl From<Bound> for Bind {
    fn from(bound: Bound) -> Self {
        match bound {
            Bound::Every => Self::Any,
            Bound::Address(address) if address.is_unspecified() => Self::Any,
            Bound::Address(address) if address.is_loopback() => Self::Loopback,
            Bound::Address(address) => Self::Address(address),
        }
    }
}

impl Bind {
    pub(crate) fn parse(host: &str) -> Option<Self> {
        Bound::parse(host).map(Self::from)
    }

    /// Which of two sockets on one port to keep: the one reachable from more
    /// places.
    pub(crate) fn reach(self) -> u8 {
        match self {
            Self::Address(_) => 0,
            Self::Loopback => 1,
            Self::Any => 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Port {
    pub number: u16,
    pub bind: Bind,
    /// The listening process's short name, bounded display text.
    pub process: String,
    /// The `HERDR_PANE_ID` of the pane that started it, only once its
    /// daemon said that pane is closed; `None` when the process did not say,
    /// the daemon did not say so, or two processes on the number disagree.
    pub pane: Option<String>,
}

impl Port {
    /// Where a browser on this machine reaches a port of this machine.
    fn url(&self) -> Option<WebUrl> {
        let name = match self.bind {
            Bind::Address(IpAddr::V6(address)) => format!("[{address}]"),
            Bind::Address(IpAddr::V4(address)) => address.to_string(),
            Bind::Any | Bind::Loopback => "localhost".into(),
        };
        WebUrl::try_from(format!("http://{name}:{}/", self.number)).ok()
    }

    /// What clicking the port does: open its page, or first tunnel to a
    /// remote one. A remote server always opens through SSH on this
    /// machine's loopback: the address SSH dials, perhaps a VPN's, says
    /// nothing about whether a browser here may reach the server directly.
    pub(crate) fn link(&self, origin: &Origin) -> Option<Link> {
        match origin {
            Origin::Local => self.url().map(Link::Page),
            Origin::Remote { target } => Some(Link::Tunnel(Key {
                target: target.clone(),
                address: match self.bind {
                    Bind::Address(address) => Some(address),
                    Bind::Any | Bind::Loopback => None,
                },
                port: self.number,
            })),
        }
    }

    /// `*:3000`, `127.0.0.1:3000`, as a tooltip names the socket.
    pub(crate) fn address(&self) -> String {
        match self.bind {
            Bind::Any => format!("*:{}", self.number),
            Bind::Loopback => format!("localhost:{}", self.number),
            Bind::Address(IpAddr::V6(address)) => format!("[{address}]:{}", self.number),
            Bind::Address(IpAddr::V4(address)) => format!("{address}:{}", self.number),
        }
    }
}

/// Where clicking a port leads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Link {
    Page(WebUrl),
    Tunnel(Key),
}

impl Link {
    /// Where it leads, as a menu names it: `localhost:5173`, or `:5173`
    /// for a remote port reached through a tunnel opened on demand.
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Page(url) => {
                let url = url.as_str();
                let url = url.split_once("://").map_or(url, |(_, rest)| rest);
                url.trim_end_matches('/').to_owned()
            }
            Self::Tunnel(key) => format!(":{}", key.port),
        }
    }
}

/// Where a browser on this machine finds the scanned host.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Origin {
    #[default]
    Local,
    /// The SSH target, which a tunnel to the host dials.
    Remote { target: String },
}

impl Origin {
    pub(crate) fn new(host: &Host) -> Self {
        match host {
            // WSL forwards a distribution's localhost ports to this machine.
            Host::Local | Host::Wsl(_) => Self::Local,
            // Never scanned: `open` refuses it.
            #[cfg(feature = "azure")]
            Host::Azure { .. } => Self::Local,
            Host::Ssh(target) => Self::Remote {
                target: target.clone(),
            },
        }
    }
}

/// The pane a listener was started in: its workspace, and the JSON API
/// socket of the daemon that owns it, empty when the process did not say.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Owner {
    pub workspace: String,
    pub socket: String,
}

/// Each workspace's ports, by owner, lowest port first.
pub(crate) type Ports = HashMap<Owner, Vec<Port>>;

/// What one scan of a host found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scan {
    pub ports: Ports,
    /// The pane each listening process was started in. Several processes
    /// can share a number, so only the process names a listener's owner.
    pub pids: HashMap<u32, Owner>,
    /// The listening processes whose daemon said the pane their
    /// environment names is closed, with that pane.
    pub closed: HashMap<u32, String>,
}

/// Parses what [`COMMAND`] printed.
pub(crate) fn parse(text: &str) -> Result<Scan> {
    let mut listeners: Vec<(u32, Port)> = Vec::new();
    let mut owners: HashMap<u32, Owner> = HashMap::new();
    let mut panes: HashMap<u32, String> = HashMap::new();
    for line in text.lines() {
        let mut words = line.splitn(4, ' ');
        match words.next() {
            Some("N") => return Err(Error::Tool.into()),
            Some("L") if listeners.len() < MAX_LISTENERS => {
                let (Some(pid), Some(address)) = (words.next(), words.next()) else {
                    continue;
                };
                let (Ok(pid), Some(port)) = (pid.parse(), listener(address, words.next())) else {
                    continue;
                };
                listeners.push((pid, port));
            }
            Some("E") => {
                let (Some(pid), Some(workspace)) = (words.next(), words.next()) else {
                    continue;
                };
                let workspace = workspace.trim_end();
                let plain = token(workspace);
                let socket = words.next().map(socket).unwrap_or_default();
                if let (Ok(pid), true) = (pid.parse(), plain) {
                    owners.insert(
                        pid,
                        Owner {
                            workspace: workspace.to_owned(),
                            socket: socket.to_owned(),
                        },
                    );
                }
            }
            Some("P") => {
                let (Some(pid), Some(pane)) = (words.next(), words.next()) else {
                    continue;
                };
                let pane = pane.trim_end();
                if let (Ok(pid), true, None) = (pid.parse(), token(pane), words.next()) {
                    panes.insert(pid, pane.to_owned());
                }
            }
            _ => {}
        }
    }
    let mut scan = Scan::default();
    for (pid, port) in listeners {
        let Some(owner) = owners.get(&pid) else {
            continue;
        };
        scan.pids.insert(pid, owner.clone());
        if let Some(pane) = panes.get(&pid) {
            scan.closed.insert(pid, pane.clone());
        }
        let port = Port {
            pane: panes.get(&pid).cloned(),
            ..port
        };
        let list = scan.ports.entry(owner.clone()).or_default();
        match list.iter_mut().find(|known| known.number == port.number) {
            // IPv4 and IPv6 sockets of one server share its number.
            Some(known) => {
                // Processes of two panes on one number: neither pane alone
                // says whether the port's server outlived its pane.
                let pane = (known.pane == port.pane)
                    .then(|| port.pane.clone())
                    .flatten();
                if port.bind.reach() > known.bind.reach() {
                    *known = port;
                }
                known.pane = pane;
            }
            None => list.push(port),
        }
    }
    for list in scan.ports.values_mut() {
        list.sort_by_key(|port| port.number);
        list.truncate(MAX_PORTS);
    }
    Ok(scan)
}

/// A process's `HERDR_SOCKET_PATH`, or empty when it is unusable. Paths
/// are compared, never opened.
pub(crate) fn socket(text: &str) -> &str {
    let text = text.trim_end();
    if text.len() <= MAX_SOCKET && !text.chars().any(char::is_control) {
        text
    } else {
        ""
    }
}

/// A short daemon token such as a workspace ID `w7V` or a pane ID `w7V:p2`.
pub(crate) fn token(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_WORKSPACE_ID
        && text.chars().all(|c| c.is_ascii_graphic() && c != '=')
}

/// The address of `127.0.0.1:3000`, `[::1]:3000`, `*%lo:3000`, as printed.
/// Only the Ports section's scan asks, and it runs on Unix.
#[cfg(unix)]
pub(crate) fn bound(address: &str) -> Option<Bound> {
    Bound::parse(address.rsplit_once(':')?.0)
}

/// `127.0.0.1:3000`, `[::1]:3000`, `*:3000`, `0.0.0.0:3000`.
pub(crate) fn listener(address: &str, process: Option<&str>) -> Option<Port> {
    let (host, number) = address.rsplit_once(':')?;
    let number: u16 = number.parse().ok().filter(|number| *number > 0)?;
    Some(Port {
        number,
        bind: Bind::parse(host)?,
        process: process
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_PROCESS)
            .collect::<String>()
            .trim()
            .to_owned(),
        pane: None,
    })
}
