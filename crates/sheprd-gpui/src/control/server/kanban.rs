//! Board reads select a window, not a repository, and never change its panel.
use super::in_window;
use crate::{
    control::protocol::{
        Call, ErrorCode, Response,
        orchestrator::{self as wire, BoardReply, OrchestratorOp},
    },
    i18n::fl,
};
use gpui::App;

pub(super) fn serve(call: Call<OrchestratorOp>, cx: &mut App) -> Response {
    let cursor = match &call.op {
        OrchestratorOp::Board { cursor } => cursor.clone(),
        OrchestratorOp::BoardOrder { .. } => None,
    };
    let cursor = match cursor.as_deref().map(parse_cursor).transpose() {
        Ok(cursor) => cursor,
        Err(()) => return Response::error(ErrorCode::InvalidRequest, fl!("kanban-invalid-cursor")),
    };
    in_window(&call.caller, cx, |view, window, cx| {
        if !view.activity_bar_shown() {
            return Response::error(ErrorCode::Unsupported, fl!("kanban-disabled"));
        }
        // Creating the entity performs no provider I/O. The host tick alone
        // supplies repository workers and starts account discovery.
        let id = view.sections_view(cx);
        view.activity.section_view = Some(id);
        let Some(board) = view.orchestrator_view(id).cloned() else {
            return pending();
        };
        if let OrchestratorOp::BoardOrder { lane, before } = &call.op {
            use crate::orchestrator::view::kanban::{error::Error, order};
            let Some(lane) = order::parse(lane) else {
                return Response::error(ErrorCode::NotFound, fl!("kanban-order-unknown"));
            };
            let before = match before.as_deref().map(order::parse) {
                Some(None) => {
                    return Response::error(ErrorCode::NotFound, fl!("kanban-order-unknown"));
                }
                value => value.flatten(),
            };
            if view.menu.page.is_some() || cx.has_active_drag() {
                return Response::error(ErrorCode::Busy, fl!("kanban-order-busy"));
            }
            let result = board.update(cx, |board, cx| {
                let _ = board.request_board(cx);
                board.order_lane(&lane, before.as_ref(), cx)
            });
            return match result {
                Ok(()) => {
                    view.collect_lane_order(&board, cx);
                    Response::Done {
                        message: fl!("kanban-order-queued"),
                    }
                }
                Err(Error::Pending) => pending(),
                Err(error) => Response::error(
                    match error {
                        Error::Busy => ErrorCode::Busy,
                        Error::UnknownLane => ErrorCode::NotFound,
                        _ => ErrorCode::Failed,
                    },
                    error.localize(),
                ),
            };
        }
        board.update(cx, |board, cx| {
            board.set_kanban_mapping(&view.config.kanban, cx);
            match board.request_board(cx) {
                Err(error) => Response::error(ErrorCode::Failed, error.localize()),
                Ok(None) => pending(),
                Ok(Some(ready)) => match &ready.pages {
                    Ok(pages) => page(pages, window.window_handle().window_id().as_u64(), cursor),
                    Err(error) => Response::error(ErrorCode::Failed, error.localize()),
                },
            }
        })
    })
}

fn pending() -> Response {
    Response::Orchestrator {
        reply: BoardReply::Pending,
    }
}

#[derive(Clone, Copy)]
struct Cursor {
    window: u64,
    revision: u64,
    offset: usize,
}

fn parse_cursor(text: &str) -> Result<Cursor, ()> {
    if text.len() > wire::MAX_CURSOR_BYTES {
        return Err(());
    }
    let mut pieces = text.split('.');
    let window = u64::from_str_radix(pieces.next().ok_or(())?, 16).map_err(|_| ())?;
    let revision = u64::from_str_radix(pieces.next().ok_or(())?, 16).map_err(|_| ())?;
    let offset = usize::from_str_radix(pieces.next().ok_or(())?, 16).map_err(|_| ())?;
    if pieces.next().is_some() {
        return Err(());
    }
    Ok(Cursor {
        window,
        revision,
        offset,
    })
}

fn page(pages: &[wire::BoardPage], window: u64, cursor: Option<Cursor>) -> Response {
    let Some(first) = pages.first() else {
        return pending();
    };
    if cursor.is_some_and(|cursor| cursor.window != window || cursor.revision != first.revision) {
        return Response::Orchestrator {
            reply: BoardReply::RestartRequired,
        };
    }
    let offset = cursor.map_or(0, |cursor| cursor.offset);
    let Some(mut page) = pages.get(offset).cloned() else {
        return Response::error(ErrorCode::InvalidRequest, fl!("kanban-invalid-cursor"));
    };
    page.next_cursor = (offset + 1 < pages.len())
        .then(|| format!("{window:x}.{:x}.{:x}", page.revision, offset + 1));
    Response::Orchestrator {
        reply: BoardReply::Page(Box::new(page)),
    }
}

#[cfg(test)]
mod tests;
