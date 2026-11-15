//! Read the same prepared board the native view shows, without showing it.
use super::{Context, Output, unexpected};
use crate::{
    control::protocol::{
        Request, Response,
        orchestrator::{BoardReply, OrchestratorOp},
    },
    i18n::fl,
};

pub(super) fn propose(
    item: String,
    lane: String,
    to: sheprd_control_protocol::BoardColumn,
    context: &Context,
) -> crate::Result<Output> {
    match context.call(
        Request::Orchestrator,
        OrchestratorOp::BoardMove { item, lane, to },
    )? {
        Response::Done { message } => Ok(Output::message(message)),
        Response::Orchestrator {
            reply: BoardReply::Pending,
        } => Ok(Output::data(fl!("kanban-pending"), &BoardReply::Pending)?.nothing()),
        response => Err(unexpected(response)),
    }
}

pub(super) fn order(
    lane: String,
    before: Option<String>,
    context: &Context,
) -> crate::Result<Output> {
    match context.call(
        Request::Orchestrator,
        OrchestratorOp::BoardOrder { lane, before },
    )? {
        Response::Done { message } => Ok(Output::message(message)),
        Response::Orchestrator {
            reply: BoardReply::Pending,
        } => Ok(Output::data(fl!("kanban-pending"), &BoardReply::Pending)?.nothing()),
        response => Err(unexpected(response)),
    }
}

pub(super) fn rank(lane: String, items: Vec<String>, context: &Context) -> crate::Result<Output> {
    match context.call(
        Request::Orchestrator,
        OrchestratorOp::BoardRank { lane, items },
    )? {
        Response::Done { message } => Ok(Output::message(message)),
        Response::Orchestrator {
            reply: BoardReply::Pending,
        } => Ok(Output::data(fl!("kanban-pending"), &BoardReply::Pending)?.nothing()),
        response => Err(unexpected(response)),
    }
}

pub(super) fn read(cursor: Option<String>, context: &Context) -> crate::Result<Output> {
    let response = context.call(Request::Orchestrator, OrchestratorOp::Board { cursor })?;
    let Response::Orchestrator { reply } = response else {
        return Err(unexpected(response));
    };
    match &reply {
        BoardReply::Pending => Ok(Output::data(fl!("kanban-pending"), &reply)?.nothing()),
        BoardReply::RestartRequired => Ok(Output::data(fl!("kanban-restart"), &reply)?.nothing()),
        BoardReply::Repos(_) => Err(unexpected(Response::Orchestrator {
            reply: reply.clone(),
        })),
        BoardReply::Page(page) => {
            let mut text = fl!("kanban-page", revision = page.revision.to_string());
            for lane in &page.lanes {
                text.push_str(&format!("\n{}", lane.key));
            }
            for card in &page.cards {
                let rank = card
                    .rank
                    .map(|rank| format!("{rank}. "))
                    .unwrap_or_default();
                text.push_str(&format!(
                    "\n{}  {}  {rank}{}",
                    card.lane, card.key, card.title
                ));
            }
            if let Some(cursor) = &page.next_cursor {
                text.push_str(&format!("\n--cursor {cursor}"));
            }
            Output::data(text, &reply)
        }
    }
}
