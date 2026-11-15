//! Finding and running the user's `artifacts` program. Every function that
//! runs it blocks: call them only on a background thread. Each run reads at
//! most [`MAX_ANSWER`] bytes of output and ends the child once it is silent
//! for longer than its bound.
use super::{Error, Result, Status};
use crate::browser::WebUrl;
use sheprd_control_protocol::protocol::artifacts::State;
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::Duration,
};

pub(crate) const NAME: &str = "artifacts";
/// The most output read from one run; `status` prints one short object.
pub(crate) const MAX_ANSWER: usize = 4096;
/// `status` checks the server's health for at most 5 seconds.
const STATUS_IDLE: Duration = Duration::from_secs(15);
/// `start` prints only at its end: its `node` and port checks, up to 30
/// seconds of health wait, and up to 12 more to end a server that failed it.
const START_IDLE: Duration = Duration::from_secs(90);
/// `off` waits up to 10 seconds for the servers to end.
const OFF_IDLE: Duration = Duration::from_secs(30);

/// `artifacts` on `PATH`, else `~/.local/bin/artifacts`, where the kit
/// links it and a desktop-started app's `PATH` often does not reach.
pub(crate) fn locate() -> Option<PathBuf> {
    let home = sheprd_client::local_path::home_dir();
    locate_in(std::env::var_os("PATH").as_deref(), home.as_deref())
}

/// Only absolute directories count, so the working directory never does.
/// The program is a script with a `#!` line, which only Unix runs.
pub(crate) fn locate_in(path: Option<&OsStr>, home: Option<&Path>) -> Option<PathBuf> {
    if !cfg!(unix) {
        return None;
    }
    let listed = path.map(std::env::split_paths).into_iter().flatten();
    let local = home.map(|home| home.join(".local").join("bin"));
    listed
        .chain(local)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(NAME))
        .find(|candidate| executable(candidate))
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(_: &Path) -> bool {
    false
}

#[derive(serde::Deserialize)]
struct Answer {
    state: State,
    url: Option<String>,
}

/// Parses `artifacts --json status`. An unknown state, a page that is not
/// http or https, or an answer over [`MAX_ANSWER`] bytes is an error.
pub(crate) fn parse_status(bytes: &[u8]) -> Result<Status> {
    if bytes.len() > MAX_ANSWER {
        return Err(Error::Oversized { limit: MAX_ANSWER });
    }
    let answer: Answer = serde_json::from_slice(bytes.trim_ascii()).map_err(Error::Answer)?;
    let url = answer
        .url
        .map(|url| WebUrl::try_from(url.as_str()))
        .transpose()
        .map_err(|error| Error::Address(Box::new(error)))?;
    Ok(Status {
        state: answer.state,
        url,
    })
}

/// The page `artifacts start` printed as its last line, when it is one.
pub(crate) fn started_page(bytes: &[u8]) -> Option<WebUrl> {
    let text = std::str::from_utf8(bytes).ok()?;
    let line = text.lines().rev().find(|line| !line.trim().is_empty())?;
    WebUrl::try_from(line.trim()).ok()
}

fn run(program: &Path, args: &[&str], idle: Duration) -> Result<Vec<u8>> {
    let mut command = sheprd_client::new_command(program);
    // The server it starts needs `node`, which a desktop launch's `PATH`
    // may not reach.
    command
        .args(args)
        .env("PATH", sheprd_client::local_path::local_path());
    let mut output = Vec::new();
    let limits = sheprd_client::ScriptLimits {
        output: MAX_ANSWER as u64,
        idle,
    };
    sheprd_client::run_script_command(
        command,
        std::io::empty(),
        &mut output,
        limits,
        &AtomicBool::new(false),
    )
    .map_err(Error::Run)?;
    Ok(output)
}

pub(crate) fn status(program: &Path) -> Result<Status> {
    parse_status(&run(program, &["--json", "status"], STATUS_IDLE)?)
}

/// Starts the local server; the page it printed, when it printed one.
pub(super) fn start(program: &Path) -> Result<Option<WebUrl>> {
    Ok(started_page(&run(program, &["start"], START_IDLE)?))
}

/// Stops each local server of this user; exits 0 also when none ran.
pub(super) fn off(program: &Path) -> Result<()> {
    run(program, &["off"], OFF_IDLE).map(drop)
}
