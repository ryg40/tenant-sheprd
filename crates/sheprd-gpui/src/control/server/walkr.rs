//! A status request waits on the existing bounded watch list, never on I/O.
//! `connect` and `disconnect` change the run of the caller's window at once.
//! No request closes the section's page: the user may still rate on it.
use super::{Watches, plain, socket, window_for};
use crate::{
    config::WalkrConfig,
    control::protocol::{
        Call, Caller, ErrorCode, Response,
        walkr::{HostSessions, Reply, RunAddress, Source, Status, WAIT_SECONDS, WalkrOp},
    },
    walkr::{Connectivity, DEMAND, Disconnected, INTERVAL, Walkr, run},
};
use gpui::App;
use std::time::{Duration, Instant};

/// The pane a run is held by. As `browser reload`, these run in a pane.
fn pane(caller: &Caller) -> Result<run::Pane, Response> {
    caller
        .pane_id
        .as_deref()
        .filter(|pane| plain(pane, 256))
        .map(|pane| run::Pane::new(pane, caller.daemon_socket.clone()))
        .ok_or_else(|| {
            Response::error(
                ErrorCode::InvalidRequest,
                "walkr connect and disconnect run in the Herdr pane that started Walkr",
            )
        })
}

/// The address is validated before anything changes, and no answer names
/// more of it than host and port.
pub(super) fn connect(caller: &Caller, address: &RunAddress, show: bool, cx: &mut App) -> Response {
    let parsed = match run::parse(address) {
        Ok(parsed) => parsed,
        Err(error) => return Response::error(ErrorCode::InvalidUrl, error.to_string()),
    };
    let handle = match window_for(caller, cx) {
        Ok(handle) => handle,
        Err(response) => return response,
    };
    let pane = match pane(caller) {
        Ok(pane) => pane,
        Err(response) => return response,
    };
    let address = parsed.host();
    handle
        .update(cx, |view, window, cx| {
            if show && !view.activity_bar_shown() {
                return Response::error(
                    ErrorCode::Unsupported,
                    "The activity bar is off: [sidebar] activity_bar = false. Connect without --show",
                );
            }
            match view.connect_walkr(parsed, pane, show, window, cx) {
                Ok(connected) => Response::Walkr {
                    reply: Reply::Connected {
                        address,
                        replaced: connected.replaced,
                    },
                },
                // The run is full: nothing changed.
                Err(error) => Response::error(ErrorCode::Busy, error.to_string()),
            }
        })
        .unwrap_or_else(|_| Response::error(ErrorCode::NoWindow, "The window closed"))
}

pub(super) fn disconnect(caller: &Caller, cx: &mut App) -> Response {
    let handle = match window_for(caller, cx) {
        Ok(handle) => handle,
        Err(response) => return response,
    };
    let pane = match pane(caller) {
        Ok(pane) => pane,
        Err(response) => return response,
    };
    handle
        .update(cx, |view, _, cx| {
            Response::done(match view.disconnect_walkr(&pane, cx) {
                Disconnected::NotHeld => "No Walkr run was connected for this pane",
                Disconnected::StillLive => "Disconnected. Another pane still holds the Walkr run.",
                Disconnected::Ended => {
                    "Disconnected. The page stays open in Sheprd until it is closed there."
                }
                Disconnected::Dropped => "Disconnected. No page was open, so the run is forgotten.",
            })
        })
        .unwrap_or_else(|_| Response::error(ErrorCode::NoWindow, "The window closed"))
}

/// Where the section's address comes from, whether a pane holds the run,
/// and the host and port. `None` with no run and no `walkr.url`. A run has
/// priority over the setting.
pub(super) fn source(walkr: &Walkr, config: &WalkrConfig) -> Option<(Source, bool, String)> {
    match (walkr.run(), &config.url) {
        (Some(run), _) => Some((Source::Run, run.live(), run.host())),
        (None, Some(url)) => Some((Source::Override, false, url.address())),
        (None, None) => None,
    }
}

/// What a status request asked for: the probe of the setting and the run,
/// and since when it waits for a list of the saved sessions.
#[derive(Debug, Default)]
pub(super) struct Asked {
    probe: Option<(WalkrConfig, Option<u64>)>,
    sessions: Option<Instant>,
}

/// The saved sessions of this computer, once a list is fresh enough: one
/// no older than a minute, else the one this request asked for, and the
/// waiting count of each counted host as it last answered. This computer
/// waits for the first answer of its reader; an SSH host that has not
/// answered yet is not listed, and one whose reads fail keeps its last count.
fn sessions(walkr: &mut Walkr, now: Instant, asked: &mut Asked) -> Option<Vec<HostSessions>> {
    let since = *asked.sessions.get_or_insert_with(|| {
        if walkr.sessions.fresh(now) {
            walkr.sessions.listed_at().unwrap_or(now)
        } else {
            walkr.sessions.want_list();
            now
        }
    });
    if !walkr.sessions.listed_since(since) {
        return None;
    }
    let local = walkr.waiting.local();
    if local.is_some_and(|local| local.tally.is_none() && local.error.is_none()) {
        return None;
    }
    let number = |count: usize| u32::try_from(count).unwrap_or(u32::MAX);
    let local = local.and_then(|local| local.tally.as_ref());
    let mut hosts = vec![HostSessions {
        host: "local".into(),
        count: number(walkr.sessions.rows().len()),
        waiting: number(local.map_or(0, |tally| tally.waiting)),
    }];
    hosts.extend(walkr.waiting.hosts().iter().filter_map(|host| {
        let crate::automations::Machine::Ssh(target) = &host.machine else {
            return None;
        };
        let tally = host.tally.as_ref()?;
        Some(HostSessions {
            host: target.clone(),
            count: number(tally.sessions.min(crate::walkr::history::pending::KEPT)),
            waiting: number(tally.waiting),
        })
    }));
    Some(hosts)
}

/// One look of a waiting status request at its window: the answer once a
/// probe result is fresh, a refusal, or `None` to look again. It keeps the
/// probe wanted until `until`, and asks for one probe of each target. With
/// no run and no `walkr.url` there is nothing to probe: the answer is the
/// source `none` once the saved sessions are counted, which the command
/// reports as nothing yet.
pub(super) fn status(
    view: &mut crate::SheprdWindow,
    until: Instant,
    asked: &mut Asked,
) -> Option<Response> {
    let now = Instant::now();
    view.walkr.demand(until);
    let sessions = sessions(&mut view.walkr, now, asked);
    if let Some(error) = view.walkr.sessions.error() {
        return Some(Response::error(ErrorCode::Failed, error.to_string()));
    }
    if asked
        .sessions
        .is_some_and(|since| view.walkr.sessions.failed_since(since))
    {
        return Some(Response::error(
            ErrorCode::Timeout,
            "Walkr did not answer in time; try again",
        ));
    }
    let config = &view.config.walkr;
    // A run has priority over the setting.
    let Some((source, live, address)) = source(&view.walkr, config) else {
        return Some(Response::Walkr {
            reply: Reply::Status(Status {
                connectivity: Connectivity::NotConfigured,
                address: String::new(),
                checked_seconds_ago: None,
                embedded: crate::browser::EMBEDDED,
                source: Source::None,
                live: false,
                sessions: sessions?,
            }),
        });
    };
    let target = (config.clone(), view.walkr.run().map(run::Run::serial));
    let age = view.walkr.age(config, now);
    if age.is_none_or(|age| age > INTERVAL) && asked.probe.as_ref() != Some(&target) {
        view.walkr.refresh();
        asked.probe = Some(target);
    }
    if let Some(error) = view.walkr.error() {
        return Some(Response::error(ErrorCode::Failed, error.to_string()));
    }
    let age = age.filter(|age| *age <= INTERVAL)?;
    Some(Response::Walkr {
        reply: Reply::Status(Status {
            connectivity: view.walkr.connectivity(),
            address,
            checked_seconds_ago: Some(age.as_secs()),
            embedded: crate::browser::EMBEDDED,
            source,
            live,
            sessions: sessions?,
        }),
    })
}

pub(super) fn serve(
    call: Call<WalkrOp>,
    incoming: socket::Incoming,
    watches: &mut Watches,
    cx: &mut App,
) {
    match &call.op {
        WalkrOp::Status {} => {}
        WalkrOp::Connect { address, show } => {
            return incoming.respond(connect(&call.caller, address, *show, cx));
        }
        WalkrOp::Disconnect {} => return incoming.respond(disconnect(&call.caller, cx)),
    }
    let handle = match window_for(&call.caller, cx) {
        Ok(handle) => handle,
        Err(response) => return incoming.respond(response),
    };
    let until = Instant::now() + DEMAND;
    let mut asked = Asked::default();
    watches.add(
        incoming,
        Duration::from_secs(WAIT_SECONDS),
        move |cx| {
            handle
                .update(cx, |view, _, _| status(view, until, &mut asked))
                .unwrap_or_else(|_| Some(Response::error(ErrorCode::NoWindow, "The window closed")))
        },
        Response::error(
            ErrorCode::Timeout,
            "Walkr did not answer in time; try again",
        ),
        cx,
    );
}
