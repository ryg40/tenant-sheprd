//! Evidence missing from old Item rows stays unknown, never inferred from runs.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{BoardColumn, config::LabelMap};
use crate::Item;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeStateProvenance {
    // keep-sorted start
    CachedTracker,
    Tracker,
    // keep-sorted end
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LabelCompleteness {
    // keep-sorted start
    Complete,
    Incomplete,
    // keep-sorted end
}

/// Facts describe the same observation as the Item, not a later or earlier read.
/// `None` preserves unknown provenance in caches predating these facts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkflowFacts {
    pub labels: Option<LabelCompleteness>,
    pub native_state: Option<NativeStateProvenance>,
    pub settled_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MappingWarning {
    // keep-sorted start
    ConflictingLabels,
    IncompleteLabels,
    UnknownLabels,
    UnknownNativeState,
    UnknownProvenance,
    // keep-sorted end
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadOnlyReason {
    // keep-sorted start
    IncompleteEvidence,
    PullRequest,
    UnknownState,
    // keep-sorted end
}

/// No variant is write approval. Even complete evidence needs fresh preflight,
/// provider capability checks, and native confirmation in later board parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveCapability {
    // keep-sorted start
    PreflightRequired,
    ReadOnly(ReadOnlyReason),
    // keep-sorted end
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    pub column: BoardColumn,
    pub move_capability: MoveCapability,
    pub warnings: Vec<MappingWarning>,
}

/// A later update alone cannot turn an open observation into a settled card.
pub(crate) fn prefer(
    incoming: &Item,
    facts: &WorkflowFacts,
    held: &Item,
    held_facts: &WorkflowFacts,
) -> bool {
    let open = |item: &Item| matches!(item.state.as_str(), "open" | "draft");
    if open(held) && super::history::settled(incoming) {
        return held.updated_at.is_some()
            && incoming.updated_at > held.updated_at
            && facts.native_state.is_some();
    }
    if open(incoming) && super::history::settled(held) {
        return incoming.updated_at.is_none()
            || incoming.updated_at >= held.updated_at
            || held_facts.native_state.is_none();
    }
    incoming.updated_at > held.updated_at
}

pub(super) fn place(item: &Item, facts: &WorkflowFacts, labels: &LabelMap) -> Placement {
    let mut warnings = Vec::new();
    if facts.native_state.is_none() {
        warnings.push(MappingWarning::UnknownProvenance);
    }
    let pull_request = item.pull_request.is_some() || item.key.native_id.starts_with("pr/");
    if pull_request {
        let column = match item.state.as_str() {
            "draft" => BoardColumn::InProgress,
            "open" => BoardColumn::InReview,
            "merged" | "closed" => BoardColumn::Done,
            _ => {
                warnings.push(MappingWarning::UnknownNativeState);
                BoardColumn::Open
            }
        };
        return Placement {
            column,
            move_capability: MoveCapability::ReadOnly(ReadOnlyReason::PullRequest),
            warnings,
        };
    }

    match facts.labels {
        Some(LabelCompleteness::Complete) => {}
        Some(LabelCompleteness::Incomplete) => warnings.push(MappingWarning::IncompleteLabels),
        None => warnings.push(MappingWarning::UnknownLabels),
    }
    let progress = item
        .labels
        .iter()
        .any(|label| label == labels.in_progress_label());
    let review = item
        .labels
        .iter()
        .any(|label| label == labels.in_review_label());
    if progress && review {
        warnings.push(MappingWarning::ConflictingLabels);
    }
    let known_state = matches!(item.state.as_str(), "open" | "closed");
    let complete =
        facts.native_state.is_some() && facts.labels == Some(LabelCompleteness::Complete);
    let column = match item.state.as_str() {
        "closed" => BoardColumn::Done,
        "open" if facts.labels == Some(LabelCompleteness::Complete) => match (progress, review) {
            (true, false) => BoardColumn::InProgress,
            (false, true) => BoardColumn::InReview,
            _ => BoardColumn::Open,
        },
        "open" => BoardColumn::Open,
        _ => {
            warnings.push(MappingWarning::UnknownNativeState);
            BoardColumn::Open
        }
    };
    Placement {
        column,
        move_capability: if !known_state {
            MoveCapability::ReadOnly(ReadOnlyReason::UnknownState)
        } else if !complete {
            MoveCapability::ReadOnly(ReadOnlyReason::IncompleteEvidence)
        } else {
            MoveCapability::PreflightRequired
        },
        warnings,
    }
}
