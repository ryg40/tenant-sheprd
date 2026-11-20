//! Taking the page's port back before a start. The program's own `start`
//! refuses while any program listens on its port, so a start first checks
//! the loopback port of its page as the program does, on a background
//! thread. A forward or tunnel that a window of this app holds in its own
//! state stops at once. Anything else is looked up in this computer's port
//! listing (`local_ports`) and stops only after a yes to the Ports
//! section's own stop question, whose yes then goes on to the start. A
//! command line proves nothing about who made a process, so a listed `ssh`
//! that looks like a forward of this app is asked about too. The question
//! names only listeners bound where they hold the probed address
//! (`holding`). After each
//! background wait the window checks again that the start may run
//! (`may_start`) before it starts, stops, or asks. Only the port of the page
//! `decide` chose is freed: the program's own loopback page, or a setting of
//! the same server (`same_server`).
// i18n: migrated
use super::{Job, launch::finish};
use crate::{
    SheprdWindow,
    browser::WebUrl,
    i18n::{Localize as _, fl},
    local_ports::{Batch, DEMAND, Listener, LocalPorts, Pending, SUPPORTED},
};
use gpui::{Context, Window};
use sheprd_control_protocol::protocol::hosts::{LOCAL_PORTS_WAIT_SECONDS, PortAction};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    num::NonZeroU16,
    path::PathBuf,
    time::{Duration, Instant},
};

/// How long a stopped forward of the app's own has to let go of the port.
pub(super) const RELEASE: Duration = Duration::from_secs(3);
/// How often the port, the listing, or a stop is looked at while it is
/// waited for.
const POLL: Duration = Duration::from_millis(100);
/// How long the Ports worker's stop may take: its grace, then SIGKILL.
const STOP_WAIT: Duration = Duration::from_secs(15);
/// The bound of the connect check; loopback answers at once.
const CONNECT: Duration = Duration::from_millis(500);
/// The address whose port is checked, as the artifacts program checks it.
const PROBED: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// A start that waits for its page's port: the page to open, its loopback
/// port, and the program file that answered the read or that the question
/// named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Freeing {
    pub(super) expected: WebUrl,
    pub(super) port: NonZeroU16,
    pub(super) program: PathBuf,
}

/// What a start does when a program listens on its page's port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Taken {
    /// Find what listens and free the port.
    Free,
    /// Free nothing: say only why nothing started.
    Refuse(Reason),
}

/// Why nothing started while a program holds the page's port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Reason {
    // keep-sorted start
    /// A stop ran, and the port is still in use.
    Still,
    /// The stop did not finish in its wait, or the Ports worker ended.
    Unfinished,
    /// The port list shows nothing that can hold the port.
    Unidentified,
    /// The port list could not be read.
    Unlisted,
    // keep-sorted end
}

impl Reason {
    /// The `$reason` of `artifacts-port-taken`.
    fn selector(self) -> &'static str {
        match self {
            Self::Still => "still",
            Self::Unfinished => "unfinished",
            Self::Unidentified => "unidentified",
            Self::Unlisted => "unlisted",
        }
    }
}

/// What the listing says of the port.
enum Holders {
    /// This user's processes that can hold it; none when the listing shows
    /// nothing that can.
    Listed(Vec<Listener>),
    /// The listing failed, is not there on this system, or did not come in
    /// time.
    Unlisted,
}

/// Where the Ports worker's stop is.
enum Stopping {
    // keep-sorted start
    /// Its outcome came back.
    Done,
    /// The worker ended without one.
    Lost,
    /// It still runs.
    Running,
    // keep-sorted end
}

/// Whether a program listens on `port` of IPv4 loopback, as the artifacts
/// program checks before it starts its server: a connection is accepted,
/// or the address cannot be bound. Blocking: a background thread's call.
fn taken(port: NonZeroU16) -> bool {
    let address = SocketAddr::from((PROBED, port.get()));
    TcpStream::connect_timeout(&address, CONNECT).is_ok() || TcpListener::bind(address).is_err()
}

/// Whether `port` is free within `wait`, looked at every [`POLL`].
/// Blocking: a background thread's call.
pub(super) fn free_within(port: NonZeroU16, wait: Duration) -> bool {
    let deadline = Instant::now() + wait;
    while taken(port) {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL);
    }
    true
}

/// The rows of `listing` whose process holds `port` of the address the
/// probe found taken ([`PROBED`]): one whose socket would make a bind of
/// that address fail, bound to it, to IPv4's wildcard, or to `*`, with
/// any device or none. A socket bound to another device (`0.0.0.0%eth0`)
/// takes no connection to the address, but the server cannot bind beside
/// it. A process bound only to another address of the loopback network
/// (`127.0.0.2`), to IPv6's loopback (`::1`) or wildcard, or to another
/// interface's address is left out: its stop cannot free the port.
pub(super) fn holding(listing: &[Listener], port: NonZeroU16) -> Vec<Listener> {
    listing
        .iter()
        .filter(|listener| {
            listener.number == port.get() && listener.bound.iter().any(|bound| bound.holds(PROBED))
        })
        .cloned()
        .collect()
}

/// The rows of a listing begun after `requested` that can hold `port`;
/// `None` until that listing is in.
fn holders(ports: &LocalPorts, port: NonZeroU16, requested: Instant) -> Option<Holders> {
    Some(match ports.fresh_listing(requested)? {
        Ok(listing) => Holders::Listed(holding(listing, port)),
        Err(_) => Holders::Unlisted,
    })
}

/// Where the stop that `signal` began is: a stop's outcome sets the
/// report, which `signal` cleared, and a worker that ends sets none.
fn stopping(ports: &LocalPorts) -> Stopping {
    match (ports.acting(), ports.report()) {
        (true, _) => Stopping::Running,
        (false, Some(_)) => Stopping::Done,
        (false, None) => Stopping::Lost,
    }
}

impl SheprdWindow {
    /// Stops each forward and tunnel of this app's windows that listens on
    /// `port` here: the app's own, so no question. Whether any stopped.
    pub(super) fn stop_own_forwards(&mut self, port: NonZeroU16, cx: &mut Context<Self>) -> bool {
        let mut stopped = self.stop_forwards_on(port.get());
        for handle in cx.windows() {
            // This window's own handle answers an error while it runs this
            // handler; `self` stood for it above.
            let Some(handle) = handle.downcast::<SheprdWindow>() else {
                continue;
            };
            stopped |= handle
                .update(cx, |view, _, cx| {
                    let stopped = view.stop_forwards_on(port.get());
                    if stopped {
                        cx.notify();
                    }
                    stopped
                })
                .unwrap_or(false);
        }
        if stopped {
            cx.notify();
        }
        stopped
    }

    fn stop_forwards_on(&mut self, port: u16) -> bool {
        let forwards = self.port_forwards.stop_local(port);
        let tunnels = self.tunnels.close_local(port);
        forwards || tunnels
    }

    /// A program still listens on the page's port after the app's own
    /// forwards stopped: finds what listens, or says why nothing started.
    pub(super) fn port_taken(
        &mut self,
        freeing: Freeing,
        taken: Taken,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Taken::Refuse(reason) = taken {
            return self.flash_port(freeing.port, reason, cx);
        }
        if !SUPPORTED {
            return self.flash_port(freeing.port, Reason::Unlisted, cx);
        }
        if let Err(error) = self.claim_artifacts(Job::Starting, cx) {
            return self.flash_artifacts(super::launch::failed(&error), cx);
        }
        // The worker stamps its scans with this clock.
        let requested = Instant::now();
        self.local_ports.demand(requested + DEMAND);
        self.local_ports.retry();
        let port = freeing.port;
        let timer = cx.background_executor().clone();
        let deadline = timer.now() + Duration::from_secs(LOCAL_PORTS_WAIT_SECONDS);
        cx.spawn_in(window, async move |this, cx| {
            let found = loop {
                timer.timer(POLL).await;
                let Ok(found) =
                    this.update(cx, |view, _| holders(&view.local_ports, port, requested))
                else {
                    break None;
                };
                match found {
                    Some(found) => break Some(found),
                    None if timer.now() >= deadline => break Some(Holders::Unlisted),
                    None => {}
                }
            };
            finish(cx);
            let Some(found) = found else {
                return;
            };
            let _ = this.update_in(cx, |view, window, cx| {
                if view.may_start(&freeing.program, cx) {
                    view.free_port(found, freeing, window, cx);
                }
            });
        })
        .detach();
    }

    /// Asks to stop what the listing found on the port. With nothing there
    /// that can hold it, the port is checked once more: it may have come
    /// free since the first check.
    fn free_port(
        &mut self,
        found: Holders,
        freeing: Freeing,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let listeners = match found {
            Holders::Listed(listeners) if !listeners.is_empty() => listeners,
            Holders::Listed(_) => {
                let Freeing {
                    expected, program, ..
                } = freeing;
                let taken = Taken::Refuse(Reason::Unidentified);
                return self.start_artifacts_then(expected, program, taken, window, cx);
            }
            Holders::Unlisted => return self.flash_port(freeing.port, Reason::Unlisted, cx),
        };
        let pending = Pending {
            action: PortAction::Stop,
            batch: Batch::Chosen,
            listeners,
        };
        if !self.ask_to_free_artifacts_port(pending, freeing, window, cx) {
            self.flash_artifacts(fl!("link-refused-dialog-open").into(), cx);
        }
    }

    /// After the Ports worker was asked to stop what listened on the port
    /// (`stopped`): waits for that stop's outcome, then starts. A stop that
    /// does not finish in [`STOP_WAIT`], or whose worker ends, starts
    /// nothing. A port still taken after the stop is only reported, so no
    /// second question follows.
    pub(crate) fn start_after_port_stop(
        &mut self,
        freeing: Freeing,
        stopped: crate::Result<()>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = stopped {
            return self.flash_artifacts(error.localize().into(), cx);
        }
        if let Err(error) = self.claim_artifacts(Job::Starting, cx) {
            return self.flash_artifacts(super::launch::failed(&error), cx);
        }
        let timer = cx.background_executor().clone();
        let deadline = timer.now() + STOP_WAIT;
        cx.spawn_in(window, async move |this, cx| {
            let done = loop {
                timer.timer(POLL).await;
                match this.update(cx, |view, _| stopping(&view.local_ports)) {
                    Ok(Stopping::Running) if timer.now() < deadline => {}
                    Ok(Stopping::Done) => break Some(true),
                    Ok(Stopping::Running | Stopping::Lost) => break Some(false),
                    Err(_) => break None,
                }
            };
            finish(cx);
            let Some(done) = done else {
                return;
            };
            let _ = this.update_in(cx, |view, window, cx| {
                if !done {
                    return view.flash_port(freeing.port, Reason::Unfinished, cx);
                }
                let Freeing {
                    expected, program, ..
                } = freeing;
                // A process the stop left alone because it could not be
                // proven to be the one asked about still holds the port.
                let unproven = view
                    .local_ports
                    .report()
                    .is_some_and(|report| report.unidentified > 0);
                let taken = Taken::Refuse(if unproven {
                    Reason::Unidentified
                } else {
                    Reason::Still
                });
                view.start_artifacts_then(expected, program, taken, window, cx);
            });
        })
        .detach();
    }

    /// Why nothing started while a program holds the page's port. The port
    /// is text, so no locale groups its digits.
    fn flash_port(&mut self, port: NonZeroU16, reason: Reason, cx: &mut Context<Self>) {
        let port = port.to_string();
        let text = fl!(
            "artifacts-port-taken",
            port = port,
            reason = reason.selector()
        );
        self.flash_artifacts(text.into(), cx);
    }
}
