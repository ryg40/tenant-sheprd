//! Typed Kanban refusals, including permission context without provider response bodies.
use super::Localize;
use crate::i18n::fl;
use sheprd_orchestrator::kanban::{Error, moves::PermissionStep};

impl Localize for Error {
    fn localize(&self) -> String {
        match self {
            Self::CrossLane => fl!("error-kanban-cross-lane"),
            Self::DuplicateSource => fl!("error-kanban-duplicate-source"),
            Self::HistoryIncomplete => fl!("error-kanban-history-incomplete"),
            Self::HistoryLimit { max } => fl!("error-kanban-history-limit", max = *max),
            Self::HistoryRecord => fl!("error-kanban-history-record"),
            Self::HistoryWindow => fl!("error-kanban-history-window"),
            Self::IdenticalLabels => fl!("error-kanban-identical-labels"),
            Self::Label { max } => fl!("error-kanban-label", max = *max),
            Self::MissingApproval => fl!("error-kanban-missing-approval"),
            Self::MissingLabel => fl!("error-kanban-missing-label"),
            Self::MoveBusy => fl!("error-kanban-move-busy"),
            Self::MovePermission { step, status } => {
                let operation = match step {
                    PermissionStep::AccountIdentity => fl!("kanban-permission-account"),
                    PermissionStep::AccountRepositories => fl!("kanban-permission-repositories"),
                    PermissionStep::IssueRead => fl!("kanban-permission-read"),
                    PermissionStep::LabelChange => fl!("kanban-permission-label"),
                    PermissionStep::OrganizationLabels => fl!("kanban-permission-organization"),
                    PermissionStep::RepositoryLabels => fl!("kanban-permission-repository-labels"),
                    PermissionStep::RepositoryRights => fl!("kanban-permission-rights"),
                    PermissionStep::StateChange => fl!("kanban-permission-state"),
                };
                fl!(
                    "kanban-permission-context",
                    operation = operation,
                    scope = step.scope(),
                    status = status
                        .map_or_else(|| fl!("kanban-permission-no-status"), |s| s.to_string())
                )
            }
            Self::ReadOnlyMove => fl!("error-kanban-read-only-move"),
            Self::Repository => fl!("error-kanban-repository"),
            Self::Route => fl!("error-kanban-route"),
            Self::Selector(error) => fl!("error-kanban-selector", error = error.localize()),
            Self::SelectorCanonical => fl!("error-kanban-selector-canonical"),
            Self::StaleMove => fl!("error-kanban-stale-move"),
            Self::TooManySources { max } => fl!("error-kanban-too-many-sources", max = *max),
            Self::UnsafeLabel => fl!("error-kanban-unsafe-label"),
            Self::UnsupportedProvider(provider) => fl!(
                "error-kanban-unsupported-provider",
                provider = provider.as_str()
            ),
        }
    }
}
