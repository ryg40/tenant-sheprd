//! Prepared move labels. An outcome never supplies a desired local column.
use crate::i18n::{errors::Localize, fl};
use sheprd_orchestrator::{
    Item, Provider,
    kanban::moves::{MovePlan, Mutation, Outcome},
    service::{MovePhase, MoveStatus},
};

pub(super) fn available(item: &Item) -> bool {
    item.key.source.provider == Provider::Forgejo
        && item.pull_request.is_none()
        && item
            .key
            .native_id
            .parse::<u64>()
            .ok()
            .is_some_and(|number| number > 0 && number.to_string() == item.key.native_id)
        && matches!(item.state.as_str(), "open" | "closed")
}

pub(super) fn label(status: &MoveStatus) -> String {
    match &status.phase {
        MovePhase::Preparing => fl!(
            "kanban-move-preparing",
            column = super::rows::column(status.destination)
        ),
        MovePhase::AwaitingConfirmation(_) => fl!("kanban-move-awaiting"),
        MovePhase::Writing => fl!("kanban-move-writing"),
        MovePhase::Refused(error) => fl!("kanban-move-refused", error = error.localize()),
        MovePhase::Cancelled => fl!("kanban-move-cancelled"),
        MovePhase::Unchanged => fl!("kanban-move-no-change"),
        MovePhase::Finished(completion) => {
            if completion
                .observation
                .as_ref()
                .is_some_and(|record| record.item != status.observed.item)
            {
                return fl!("kanban-move-changed-elsewhere");
            }
            match &completion.outcome {
                Outcome::Refused(error) => fl!("kanban-move-refused", error = error.localize()),
                Outcome::Partial { error, .. } => {
                    fl!("kanban-move-partial", error = error.localize())
                }
                Outcome::Unknown { error, .. } => {
                    fl!("kanban-move-unknown-outcome", error = error.localize())
                }
                Outcome::Verified => fl!("kanban-move-verified"),
                Outcome::Observed => fl!("kanban-move-observed"),
            }
        }
    }
}

pub(super) fn mutation(plan: &MovePlan, mutation: &Mutation) -> String {
    let name = |id| plan.label_name(id).unwrap_or_default().to_owned();
    match mutation {
        Mutation::AddLabel(id) => fl!("kanban-move-add", label = name(id)),
        Mutation::RemoveLabel(id) => fl!("kanban-move-remove", label = name(id)),
        Mutation::SetState(state) => fl!(
            "kanban-move-state",
            from = plan.original().item.state,
            to = state.as_str()
        ),
    }
}
