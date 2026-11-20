//! The `[artifacts]` table and the local artifact server. With `url` set, or
//! with the user's `artifacts` program installed (`program::locate`), the
//! activity bar shows an Artifacts button. With the program, a click reads
//! `artifacts --json status` on a background thread first: a server that
//! answers opens its page, a stopped local server is started with
//! `artifacts start` and then opens its page, a local server that holds on
//! but does not answer gets `artifacts off` before that start, and anything
//! else flashes why. Before a start, the port of the page is taken back
//! from whatever listens on it (`port`): the app's own forwards at once, any
//! other program only after the Ports question's yes. A `url` the program
//! does not serve, and a program that gives no answer, leave the click to
//! open `url` as before (`decide`). Without the program a click opens
//! `url`. `sheprd artifacts start` and `off` only ask (`menu/artifacts.rs`).
//! Sheprd starts the server only on a click of the button or a yes to that
//! question, stops it only on such a click or after a yes, and leaves it
//! running when it quits. `WebUrl` already refuses anything but http and
//! https.
use crate::browser::WebUrl;
use gpui::{App, Global};
pub(crate) use sheprd_control_protocol::protocol::artifacts::State;
use std::path::PathBuf;

mod error;
mod launch;
mod port;
pub(crate) mod program;

pub(crate) use error::Error;
pub(crate) use port::Freeing;
pub(crate) type Result<T, E = Error> = std::result::Result<T, E>;

#[cfg(test)]
pub(crate) mod tests;

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Config {
    /// The artifacts page. Unset, the program's browse page is used; with
    /// neither, the button is off the bar.
    pub(crate) url: Option<WebUrl>,
}

/// The program's answer to `artifacts --json status`, parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Status {
    pub(crate) state: State,
    pub(crate) url: Option<WebUrl>,
}

impl Status {
    /// The page to open: the setting when set, else the program's.
    pub(crate) fn page(&self, config: &Config) -> Option<WebUrl> {
        config.url.clone().or_else(|| self.url.clone())
    }
}

/// What a click or `sheprd artifacts start` does with a status answer.
/// The caller chooses how [`Action::Start`] and [`Action::Restart`] run:
/// the click runs them at once, the command only asks the user, and
/// `sheprd artifacts open` starts nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// The server answers: open its page.
    Open(WebUrl),
    /// A local server is configured and stopped: start it, then open this
    /// page.
    Start(WebUrl),
    /// The program's local server holds on but does not answer: stop it
    /// with `off`, start it, then open this page.
    Restart(WebUrl),
    /// Nothing can open: say why.
    Refuse(Refusal),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    Unreachable,
    Unconfigured,
    /// Neither the setting nor the program names a page.
    NoAddress,
}

/// The pure decision of a click, without a window. `status` is `None` when
/// the read failed. The setting wins: the program decides only without it,
/// or when the program's page has the setting's origin (scheme, host, and
/// port); else the click opens the setting as before the program existed.
/// `None` back: the read failed and no setting names a page, so only the
/// read's error is left to show.
pub(crate) fn decide(status: Option<&Status>, config: &Config) -> Option<Action> {
    if let Some(setting) = &config.url {
        let program_decides = status.is_some_and(|status| {
            (matches!(
                status.state,
                State::Running | State::Remote | State::Stopped
            ) || restarts(status))
                && status
                    .url
                    .as_ref()
                    .is_some_and(|url| same_server(url, setting))
        });
        if !program_decides {
            return Some(Action::Open(setting.clone()));
        }
    }
    let status = status?;
    Some(match (status.state, status.page(config)) {
        (State::Unreachable, Some(url)) if restarts(status) => Action::Restart(url),
        (State::Unreachable, _) => Action::Refuse(Refusal::Unreachable),
        (State::Unconfigured, _) => Action::Refuse(Refusal::Unconfigured),
        (_, None) => Action::Refuse(Refusal::NoAddress),
        (State::Running | State::Remote, Some(url)) => Action::Open(url),
        (State::Stopped, Some(url)) => Action::Start(url),
    })
}

/// Whether `status` is the program's own local server holding on without
/// an answer: `unreachable` at a loopback page. An `unreachable` page on
/// another machine is a remote server, which `off` and `start` leave alone.
fn restarts(status: &Status) -> bool {
    status.state == State::Unreachable
        && status
            .url
            .as_ref()
            .is_some_and(|url| url.loopback_port().is_some())
}

/// What `sheprd artifacts start` does with a status answer: the click's
/// rule, except that a stopped or unanswering local server is always asked
/// about, also when the setting names another server, since the user named
/// `start`. The question then names the program's page, the server a yes
/// starts.
pub(crate) fn decide_start(status: &Status, config: &Config) -> Option<Action> {
    match (status.state, &status.url, &config.url) {
        (State::Stopped, Some(page), Some(setting)) if !same_server(page, setting) => {
            Some(Action::Start(page.clone()))
        }
        (State::Unreachable, Some(page), Some(setting))
            if restarts(status) && !same_server(page, setting) =>
        {
            Some(Action::Restart(page.clone()))
        }
        _ => decide(Some(status), config),
    }
}

/// Why the artifact server question did not open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotAsked {
    /// Another page or dialog holds the window.
    DialogOpen,
    /// The program is no longer found, or is another file than the one
    /// that answered the status, as after a config reload.
    NoProgram,
}

/// Whether two addresses name one server: the same origin, or the same
/// scheme and port on this machine's loopback, since `localhost` and
/// `127.0.0.1` are two origins but the one local server.
pub(crate) fn same_server(a: &WebUrl, b: &WebUrl) -> bool {
    let scheme = |url: &WebUrl| {
        url.as_str()
            .split_once(':')
            .map(|(scheme, _)| scheme.to_owned())
    };
    a.origin() == b.origin()
        || (scheme(a) == scheme(b)
            && a.loopback_port().is_some()
            && a.loopback_port() == b.loopback_port())
}

/// What `sheprd artifacts off` does with the program's state. Only
/// [`Stop::Ask`] leads anywhere, and it only asks the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stop {
    /// A local server runs, or may still hold its port: ask to stop it.
    Ask,
    /// No local server runs: nothing to stop.
    NothingRuns,
    /// The origin is a remote server, which `off` does not stop.
    Remote,
    Unconfigured,
}

pub(crate) fn decide_stop(state: State) -> Stop {
    match state {
        State::Running | State::Unreachable => Stop::Ask,
        State::Stopped => Stop::NothingRuns,
        State::Remote => Stop::Remote,
        State::Unconfigured => Stop::Unconfigured,
    }
}

/// What the artifact server question asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ask {
    /// Start the local server, then open this page.
    Start(WebUrl),
    /// Stop the local server that does not answer, start it, then open this
    /// page.
    Restart(WebUrl),
    /// Stop each local server.
    Stop,
}

/// The question as the window shows it: what it asks and the program file
/// a yes runs, found when it was asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Question {
    pub(crate) ask: Ask,
    pub(crate) program: PathBuf,
    /// `program` from the home folder, as `~/.local/bin/artifacts`.
    pub(crate) shown: String,
}

/// The child of the program that runs, at most one in the app at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Job {
    Reading,
    Starting,
    Stopping,
}

/// What the app knows of the program: where it is, read off the UI thread
/// at start and on each config reload, and the child that runs.
#[derive(Debug, Default)]
pub(crate) struct Launcher {
    program: Option<PathBuf>,
    job: Option<Job>,
}

impl Global for Launcher {}

/// Whether the bar shows the button: a page is set or the program is there.
pub(crate) fn shown(config: &Config, cx: &App) -> bool {
    config.url.is_some()
        || cx
            .try_global::<Launcher>()
            .is_some_and(|launcher| launcher.program.is_some())
}

pub(crate) fn program_path(cx: &App) -> Option<PathBuf> {
    cx.try_global::<Launcher>()?.program.clone()
}

/// Puts `program` in place as the found program, for tests outside the
/// module, as `find_program` would.
#[cfg(test)]
pub(crate) fn found_for_test(program: PathBuf, cx: &mut App) {
    cx.default_global::<Launcher>().program = Some(program);
}

/// Whether a child of the program runs, for the button's busy pulse.
pub(crate) fn busy(cx: &App) -> bool {
    cx.try_global::<Launcher>()
        .is_some_and(|launcher| launcher.job.is_some())
}

/// Looks for the program on a background thread, and redraws the windows
/// when what it found changed. Runs nothing.
pub(crate) fn find_program(cx: &mut App) {
    let found = cx.background_executor().spawn(async { program::locate() });
    cx.spawn(async move |cx| {
        let program = found.await;
        cx.update(|cx| {
            let launcher = cx.default_global::<Launcher>();
            if launcher.program != program {
                launcher.program = program;
                redraw(cx);
            }
        });
    })
    .detach();
}

/// Claims the one job slot; `false` while another child runs.
fn claim(job: Job, cx: &mut App) -> bool {
    let launcher = cx.default_global::<Launcher>();
    if launcher.job.is_some() {
        return false;
    }
    launcher.job = Some(job);
    redraw(cx);
    true
}

fn release(cx: &mut App) {
    cx.default_global::<Launcher>().job = None;
    redraw(cx);
}

/// The bar sits in each window's cached sidebar, which redraws only when
/// its window is notified.
fn redraw(cx: &mut App) {
    for handle in cx.windows() {
        if let Some(handle) = handle.downcast::<crate::SheprdWindow>() {
            let _ = handle.update(cx, |_, _, cx| cx.notify());
        }
    }
}
