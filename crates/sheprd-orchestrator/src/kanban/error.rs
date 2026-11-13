//! Refusals at the board boundary, before any move can be planned.

use crate::Provider;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error("A card cannot move to another tracker lane")]
    CrossLane,
    #[error("kanban.sources contains the same tracker repository more than once")]
    DuplicateSource,
    #[error("The history page set is incomplete; the last successful sample is kept")]
    HistoryIncomplete,
    #[error("A history read exceeds its input limit of {max} records")]
    HistoryLimit { max: usize },
    #[error("History needs settled tracker records with update times")]
    HistoryRecord,
    #[error("The history cutoff must fall within the last 30 days")]
    HistoryWindow,
    #[error("Kanban label names must be distinct")]
    IdenticalLabels,
    #[error("Kanban labels need 1 to {max} UTF-8 bytes, without commas or control characters")]
    Label { max: usize },
    #[error("The move has no current native confirmation")]
    MissingApproval,
    #[error("The workflow label must already exist")]
    MissingLabel,
    #[error("A move is pending or the move limit is reached")]
    MoveBusy,
    #[error("Move permission refused during {step} (HTTP status: {status:?}; required scope: {}); cause unproved", step.scope())]
    MovePermission {
        step: PermissionStep,
        status: Option<u16>,
    },
    #[error("Pull requests and incomplete tracker evidence cannot be moved")]
    ReadOnlyMove,
    #[error("Kanban sources must name a tracker host and repository")]
    Repository,
    #[error("The item does not belong to the worker's tracker source")]
    Route,
    #[error("Invalid Kanban source selector: {0}")]
    Selector(#[source] Box<crate::Error>),
    #[error("Kanban source selectors must use their exact canonical form")]
    SelectorCanonical,
    #[error("The approved move changed; prepare and confirm a new plan")]
    StaleMove,
    #[error("kanban.sources permits at most {max} overrides")]
    TooManySources { max: usize },
    #[error("The workflow label identity is invalid or could remove unrelated labels")]
    UnsafeLabel,
    #[error("Kanban does not support {0:?}")]
    UnsupportedProvider(Provider),
    // keep-sorted end
}

/// Fixed diagnostic context only: never a credential, header, or response body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionStep {
    // keep-sorted start
    AccountIdentity,
    AccountRepositories,
    IssueRead,
    LabelChange,
    OrganizationLabels,
    RepositoryLabels,
    RepositoryRights,
    StateChange,
    // keep-sorted end
}

impl PermissionStep {
    /// Scope required by the route, not evidence that the token lacks it.
    pub fn scope(self) -> &'static str {
        match self {
            Self::AccountIdentity => "read:user",
            Self::AccountRepositories => "read:user, read:repository",
            Self::IssueRead | Self::RepositoryLabels => "read:issue",
            Self::LabelChange | Self::StateChange => "write:issue",
            Self::OrganizationLabels => "read:organization",
            Self::RepositoryRights => "read:repository",
        }
    }
}

impl std::fmt::Display for PermissionStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::AccountIdentity => "account identity",
            Self::AccountRepositories => "account repositories",
            Self::IssueRead => "issue read",
            Self::LabelChange => "label change",
            Self::OrganizationLabels => "organization labels",
            Self::RepositoryLabels => "repository labels",
            Self::RepositoryRights => "repository rights",
            Self::StateChange => "state change",
        })
    }
}
