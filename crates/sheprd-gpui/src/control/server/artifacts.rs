//! `sheprd artifacts`: each operation reads the status through the window's
//! own path (the button's job slot), then answers. `start` and `off` only
//! open the app's question and answer at once; the user confirms there, and
//! the yes of `start` frees the server's port as a click does. `open`
//! starts nothing.
use super::{Watches, socket, window_for};
use crate::{
    SheprdWindow,
    artifacts::{
        Action, Error, NotAsked, Refusal, Result, Status, Stop, decide, decide_start, decide_stop,
        program_path,
    },
    control::protocol::{
        Call, ErrorCode, Response,
        artifacts::{ArtifactsOp, Reply, Status as WireStatus, WAIT_SECONDS},
    },
};
use gpui::{App, Context, Window};
use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Duration};

// Unix only, as the fake program is a shell script.
#[cfg(all(test, unix))]
mod tests;

pub(super) fn serve(
    call: Call<ArtifactsOp>,
    incoming: socket::Incoming,
    watches: &mut Watches,
    cx: &mut App,
) {
    let handle = match window_for(&call.caller, cx) {
        Ok(handle) => handle,
        Err(response) => return incoming.respond(response),
    };
    let answer: Rc<RefCell<Option<Response>>> = Rc::default();
    let slot = answer.clone();
    let op = call.op;
    let asked = handle.update(cx, move |view, window, cx| {
        let Some(program) = program_path(cx) else {
            // Without the program the page is the setting, as the button's.
            *slot.borrow_mut() = Some(if op == (ArtifactsOp::Open {}) {
                open_setting(view, window, cx)
            } else {
                failure(&Error::NoProgram)
            });
            return;
        };
        // The question names the file that answered, and its yes runs it.
        let read = program.clone();
        view.read_artifacts_status(read, window, cx, move |view, status, window, cx| {
            *slot.borrow_mut() = Some(answer_for(&op, status, program, view, window, cx));
        });
    });
    if asked.is_err() {
        return incoming.respond(Response::error(ErrorCode::NoWindow, "The window closed"));
    }
    watches.add(
        incoming,
        Duration::from_secs(WAIT_SECONDS),
        move |_| answer.borrow_mut().take(),
        Response::error(
            ErrorCode::Timeout,
            "The artifacts program did not answer in time",
        ),
        cx,
    );
}

fn open_setting(
    view: &mut SheprdWindow,
    window: &mut Window,
    cx: &mut Context<SheprdWindow>,
) -> Response {
    match view.config.artifacts.url.clone() {
        Some(url) => opened(url, view, window, cx),
        None => failure(&Error::NoProgram),
    }
}

fn answer_for(
    op: &ArtifactsOp,
    status: Result<Status>,
    program: PathBuf,
    view: &mut SheprdWindow,
    window: &mut Window,
    cx: &mut Context<SheprdWindow>,
) -> Response {
    // The click's rule: a setting the program does not serve opens also
    // when the read failed, but not while another run holds the slot.
    let action = decide(status.as_ref().ok(), &view.config.artifacts);
    let status = match (status, op) {
        (Ok(status), _) => status,
        (Err(error), ArtifactsOp::Open {}) if action.is_some() && !matches!(error, Error::Busy) => {
            return open_action(action, view, window, cx);
        }
        (Err(error), _) => return failure(&error),
    };
    match op {
        ArtifactsOp::Status {} => Response::Artifacts {
            reply: Reply::Status(wire_status(&status, &view.config.artifacts)),
        },
        ArtifactsOp::Start {} => match decide_start(&status, &view.config.artifacts) {
            Some(Action::Open(url)) => Response::done(format!(
                "Nothing to start; the Artifacts button opens {}",
                url.as_str()
            )),
            // A command can come from an agent, so unlike the click it
            // only asks; the user's yes starts the server.
            Some(Action::Start(url)) => {
                let address = url.as_str().to_owned();
                if let Err(not_asked) = view.ask_to_start_artifacts(url, program, window, cx) {
                    return not_asked_response(not_asked);
                }
                super::work::bring_forward(window, cx);
                Response::done(format!(
                    "Asked to start the artifact server for {address}; nothing starts until the user confirms in Sheprd"
                ))
            }
            Some(Action::Restart(url)) => {
                let address = url.as_str().to_owned();
                if let Err(not_asked) = view.ask_to_restart_artifacts(url, program, window, cx) {
                    return not_asked_response(not_asked);
                }
                super::work::bring_forward(window, cx);
                Response::done(format!(
                    "Asked to restart the artifact server for {address}, which does not answer; nothing restarts until the user confirms in Sheprd"
                ))
            }
            Some(Action::Refuse(refusal)) => refused(refusal),
            None => refused(Refusal::NoAddress),
        },
        ArtifactsOp::Off {} => match decide_stop(status.state) {
            Stop::Ask => {
                if let Err(not_asked) = view.ask_to_stop_artifacts(program, window, cx) {
                    return not_asked_response(not_asked);
                }
                super::work::bring_forward(window, cx);
                Response::done(
                    "Asked to stop the local artifact server; nothing stops until the user confirms in Sheprd",
                )
            }
            Stop::NothingRuns => Response::done("No local artifact server runs"),
            Stop::Remote => Response::error(
                ErrorCode::Unsupported,
                "The artifacts origin is a remote server; off stops only a local server",
            ),
            Stop::Unconfigured => refused(Refusal::Unconfigured),
        },
        ArtifactsOp::Open {} => open_action(action, view, window, cx),
    }
}

fn open_action(
    action: Option<Action>,
    view: &mut SheprdWindow,
    window: &mut Window,
    cx: &mut Context<SheprdWindow>,
) -> Response {
    match action {
        Some(Action::Open(url)) => opened(url, view, window, cx),
        Some(Action::Start(_)) => Response::error(
            ErrorCode::Failed,
            "The artifact server is stopped; sheprd artifacts start asks the user to start it",
        ),
        Some(Action::Restart(_)) => Response::error(
            ErrorCode::Failed,
            "The artifact server does not answer; sheprd artifacts start asks the user to restart it",
        ),
        Some(Action::Refuse(refusal)) => refused(refusal),
        None => refused(Refusal::NoAddress),
    }
}

fn opened(
    url: crate::browser::WebUrl,
    view: &mut SheprdWindow,
    window: &mut Window,
    cx: &mut Context<SheprdWindow>,
) -> Response {
    let address = url.as_str().to_owned();
    if view.open_artifacts_page(url, window, cx).is_some() {
        Response::done(format!("Opened {address}"))
    } else {
        Response::error(
            ErrorCode::WorkspaceNotFound,
            "Open a workspace in the window first",
        )
    }
}

/// The state beside the page of the server it describes: the program's,
/// or the setting when the program names none. `opens` is the button's
/// page when it is another one.
fn wire_status(status: &Status, config: &crate::artifacts::Config) -> WireStatus {
    let button = status.page(config);
    let url = status.url.clone().or_else(|| button.clone());
    let opens = button.filter(|button| Some(button) != url.as_ref());
    WireStatus {
        state: status.state,
        url: url.map(|url| url.as_str().to_owned()),
        opens: opens.map(|url| url.as_str().to_owned()),
    }
}

/// A program that is gone answers as one that is not installed.
fn not_asked_response(not_asked: NotAsked) -> Response {
    match not_asked {
        NotAsked::DialogOpen => Response::error(
            ErrorCode::WindowBusy,
            "A dialog is open in the window; close it first",
        ),
        NotAsked::NoProgram => failure(&Error::NoProgram),
    }
}

fn refused(refusal: Refusal) -> Response {
    Response::error(
        ErrorCode::Failed,
        match refusal {
            Refusal::Unreachable => {
                "The artifact server does not answer; run artifacts status for details"
            }
            Refusal::Unconfigured => "The artifacts program is not set up; run artifacts setup",
            Refusal::NoAddress => "Neither artifacts.url nor the artifacts program names a page",
        },
    )
}

/// The program's own reason is untrusted: one bounded line of it.
fn failure(error: &Error) -> Response {
    let code = match error {
        Error::NoProgram => ErrorCode::Unsupported,
        Error::Busy => ErrorCode::Busy,
        _ => ErrorCode::Failed,
    };
    let message = match (error, error.program_reason()) {
        (Error::NoProgram, _) if !cfg!(unix) => {
            "Sheprd looks for the artifacts program only on Linux and macOS".to_owned()
        }
        (Error::NoProgram, _) => {
            "The artifacts program is not installed: Sheprd looks on PATH and in ~/.local/bin"
                .to_owned()
        }
        (_, Some(reason)) => format!("{error}: {reason}"),
        (_, None) => error.to_string(),
    };
    Response::error(code, message)
}
