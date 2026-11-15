//! The repositories the orchestrator's sections show.
use super::{Context, Output, unexpected};
use crate::{
    control::protocol::{
        Request, Response,
        orchestrator::{BoardReply, OrchestratorOp},
    },
    i18n::fl,
};

pub(super) fn list(context: &Context) -> crate::Result<Output> {
    let response = context.call(Request::Orchestrator, OrchestratorOp::Repos)?;
    let Response::Orchestrator {
        reply: BoardReply::Repos(repos),
    } = &response
    else {
        return Err(unexpected(response));
    };
    let mut text = if repos.selected {
        fl!("orchestrator-repos-listed-some")
    } else {
        fl!("orchestrator-repos-listed-all")
    };
    for repo in &repos.repos {
        let state = match (repo.active, repo.new) {
            (true, _) => fl!("orchestrator-repos-state-active"),
            (false, true) => fl!("orchestrator-repos-state-new"),
            (false, false) => fl!("orchestrator-repos-state-hidden"),
        };
        text.push_str(&format!("\n{state}  {}  {}", repo.key, repo.host));
    }
    if repos.truncated {
        text.push_str(&format!("\n{}", fl!("orchestrator-repos-truncated")));
    }
    Output::data(text, repos)
}

pub(super) fn set(keys: Option<Vec<String>>, context: &Context) -> crate::Result<Output> {
    match context.call(Request::Orchestrator, OrchestratorOp::ReposSet { keys })? {
        Response::Done { message } => Ok(Output::message(message)),
        response => Err(unexpected(response)),
    }
}
