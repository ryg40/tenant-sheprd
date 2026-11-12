//! `files`: the file explorer of the request's window.
use super::{Watches, in_window, not_found, socket, window_for};
use crate::{
    SheprdWindow,
    control::protocol::{
        Call, ErrorCode, Response,
        files::{FilesOp, MAX_PATH, REVEAL_WAIT_SECONDS, Reply},
    },
    file_tree::Root,
};
use gpui::App;
use std::{path::Path, time::Duration};

fn panel(view: &SheprdWindow) -> Response {
    let root = match view.file_tree.root() {
        Root::Ready(root) => Some(root.display().to_string()),
        _ => None,
    };
    Response::Files {
        reply: Reply::Panel {
            shown: view.file_tree.is_open(),
            root,
            remote: *view.file_tree.root() == Root::Remote,
        },
    }
}

/// Selects `path` in the window's explorer, or says why it cannot.
fn reveal(view: &mut SheprdWindow, path: &Path, cx: &mut gpui::Context<SheprdWindow>) -> Response {
    let root = match view.file_tree.root() {
        Root::Ready(root) => root.display().to_string(),
        Root::Remote => {
            return Response::error(
                ErrorCode::Unsupported,
                "Files are not available for a remote host yet",
            );
        }
        Root::None | Root::Locating => {
            return not_found("The window's focused pane has no folder to list");
        }
        Root::Failed(error) => return Response::error(ErrorCode::Failed, error.clone()),
    };
    match view.reveal_file(path, cx) {
        Ok(path) => Response::Files {
            reply: Reply::Revealed { root, path },
        },
        Err(error) => not_found(format!("{error}: the explorer lists {root}")),
    }
}

pub(super) fn serve(
    call: Call<FilesOp>,
    incoming: socket::Incoming,
    watches: &mut Watches,
    cx: &mut App,
) {
    let Call { caller, op } = call;
    let path = match op {
        FilesOp::Panel { switch } => {
            return incoming.respond(in_window(&caller, cx, |view, window, cx| {
                if let Some(switch) = switch {
                    let show = switch.apply(view.file_tree.is_open());
                    view.set_file_tree_shown(show, window, cx);
                }
                panel(view)
            }));
        }
        FilesOp::Reveal { path } => path,
    };
    let valid = !path.is_empty()
        && path.len() <= MAX_PATH
        && !path.chars().any(char::is_control)
        && Path::new(&path).is_absolute();
    if !valid {
        return incoming.respond(Response::error(
            ErrorCode::InvalidRequest,
            "The path must be absolute and without control characters",
        ));
    }
    let handle = match window_for(&caller, cx) {
        Ok(handle) => handle,
        Err(response) => return incoming.respond(response),
    };
    let closed = || Response::error(ErrorCode::NoWindow, "The window closed");
    if handle
        .update(cx, |view, window, cx| {
            view.set_file_tree_shown(true, window, cx)
        })
        .is_err()
    {
        return incoming.respond(closed());
    }
    // The checkout is found off the UI thread; the file is revealed once
    // the window knows which folder it lists.
    watches.add(
        incoming,
        Duration::from_secs(REVEAL_WAIT_SECONDS),
        move |cx| {
            handle
                .update(cx, |view, _, cx| {
                    (!view.file_tree.settling()).then(|| reveal(view, Path::new(&path), cx))
                })
                .unwrap_or_else(|_| Some(closed()))
        },
        Response::error(
            ErrorCode::Timeout,
            "The window did not find the checkout in time",
        ),
        cx,
    );
}
