//! `files`: show, hide, or report the file explorer, and reveal a file in
//! it.
use super::{Context, Output, unexpected};
use crate::{
    cli::control::FilesCommand,
    control::protocol::{
        ErrorCode, Request, Response,
        files::{FilesOp, Reply},
    },
};
use std::path::Path;

fn rejected(code: ErrorCode, message: impl Into<String>) -> crate::Error {
    crate::control::Error::Rejected {
        code,
        message: message.into(),
    }
    .into()
}

/// The app compares the path with the folder it lists, so it gets an
/// absolute path with its links resolved here, where the caller's
/// directory is known.
fn resolved(path: &Path) -> crate::Result<String> {
    path.canonicalize()
        .map_err(|error| {
            rejected(
                ErrorCode::NotFound,
                format!("Cannot find {}: {error}", path.display()),
            )
        })?
        .into_os_string()
        .into_string()
        .map_err(|_| rejected(ErrorCode::InvalidRequest, "The path must be UTF-8"))
}

pub(super) fn run(command: FilesCommand, context: &Context) -> crate::Result<Output> {
    let op = match command {
        FilesCommand::Panel { switch } => FilesOp::Panel { switch },
        FilesCommand::Reveal { path } => FilesOp::Reveal {
            path: resolved(&path)?,
        },
    };
    match context.call(Request::Files, op)? {
        Response::Files {
            reply:
                Reply::Panel {
                    shown,
                    root,
                    remote,
                },
        } => {
            let json = serde_json::json!({ "shown": shown, "root": root, "remote": remote });
            let state = if shown { "shows" } else { "is hidden" };
            let text = match (&root, remote) {
                (_, true) => format!("The file explorer {state}; a remote host is not listed"),
                (Some(root), false) => format!("The file explorer {state}: {root}"),
                (None, false) => format!("The file explorer {state}"),
            };
            Output::data(text, &json)
        }
        Response::Files {
            reply: Reply::Revealed { root, path },
        } => Output::data(
            format!("Showing {path} in {root}"),
            &serde_json::json!({ "root": root, "path": path, "shown": true }),
        ),
        response => Err(unexpected(response)),
    }
}
