//! `orchestrator repos`: the window's active repositories. Reading them
//! starts no worker; setting them saves the window's chrome as the picker does.
use super::in_window;
use crate::{
    control::protocol::{
        Call, ErrorCode, Response,
        orchestrator::{self as wire, BoardReply, MAX_PAGE_BYTES, MAX_REPO_KEYS, OrchestratorOp},
    },
    i18n::fl,
    orchestrator::view::repo_select::{Choice, Entry, error::Error},
};
use gpui::App;

pub(super) fn serve(call: Call<OrchestratorOp>, cx: &mut App) -> Response {
    let keys = match &call.op {
        OrchestratorOp::ReposSet { keys: Some(keys) } if !wire::repo_keys_bounded(keys) => {
            return Response::error(
                ErrorCode::InvalidRequest,
                fl!("orchestrator-repos-too-many"),
            );
        }
        OrchestratorOp::ReposSet { keys } => Some(keys.clone()),
        _ => None,
    };
    in_window(&call.caller, cx, |view, _, cx| {
        if !view.activity_bar_shown() {
            return Response::error(ErrorCode::Unsupported, fl!("orchestrator-repos-disabled"));
        }
        let id = view.sections_view(cx);
        view.activity.section_view = Some(id);
        let Some(sections) = view.orchestrator_view(id).cloned() else {
            return Response::Orchestrator {
                reply: BoardReply::Pending,
            };
        };
        let stored = &view.orchestrator_repos;
        let (selection, failed) = (stored.selection.clone(), stored.failed.get());
        sections.update(cx, |sections, cx| {
            sections.set_repo_selection(selection.as_ref(), failed, cx)
        });
        let Some(keys) = keys else {
            let entries = sections.read(cx).repo_entries().to_vec();
            return list(entries, view.orchestrator_repos.selection.is_some());
        };
        let choice = keys.map_or(Choice::All, Choice::Only);
        match sections.update(cx, |sections, cx| sections.choose_repos(choice, cx)) {
            Ok(()) => {
                view.collect_repo_selection(&sections, cx);
                Response::Done {
                    message: fl!("orchestrator-repos-set"),
                }
            }
            Err(Error::TooMany) => Response::error(
                ErrorCode::InvalidRequest,
                fl!("orchestrator-repos-too-many"),
            ),
            Err(Error::Unknown) => {
                Response::error(ErrorCode::NotFound, fl!("orchestrator-repos-unknown"))
            }
        }
    })
}

/// At most [`MAX_REPO_KEYS`] entries and [`MAX_PAGE_BYTES`] of JSON, the
/// reply's own fields and envelope included.
pub(super) fn list(entries: Vec<Entry>, selected: bool) -> Response {
    let reply = |repos: Vec<wire::Repo>, truncated: bool| Response::Orchestrator {
        reply: BoardReply::Repos(wire::Repos {
            selected,
            repos,
            truncated,
        }),
    };
    let envelope = serde_json::to_vec(&reply(Vec::new(), true))
        .map_or(MAX_PAGE_BYTES, |encoded| encoded.len());
    let mut repos = Vec::new();
    let mut bytes = envelope;
    let mut truncated = false;
    for entry in entries {
        let repo = wire::Repo {
            key: entry.key,
            name: entry.name,
            host: entry.host,
            active: entry.active,
            new: entry.new,
        };
        let size = serde_json::to_vec(&repo).map_or(usize::MAX, |encoded| encoded.len() + 1);
        if repos.len() == MAX_REPO_KEYS || bytes + size > MAX_PAGE_BYTES {
            truncated = true;
            break;
        }
        bytes += size;
        repos.push(repo);
    }
    reply(repos, truncated)
}
