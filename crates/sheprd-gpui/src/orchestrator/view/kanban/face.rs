//! What a card shows beyond its key and title, prepared on the background
//! executor from the snapshot alone. Nothing here reads a provider.
use super::super::{
    look::{BLUE, CYAN, GREEN, MAGENTA, RED, YELLOW},
    pr_cells,
    rows::Status,
};
use crate::{
    i18n::fl,
    orchestrator::{CheckState, Item, PrStatus},
};
use chrono::{DateTime, Utc};
use gpui::SharedString;

/// Chips a card draws before it says how many more labels the item has.
pub(super) const MAX_LABELS: usize = 3;
/// Bytes of one chip's text; the chip also truncates to its width.
const MAX_LABEL_BYTES: usize = 64;
const HUES: [usize; 6] = [BLUE, GREEN, YELLOW, RED, MAGENTA, CYAN];

#[derive(Clone)]
pub(in crate::orchestrator::view) struct Chip {
    pub hue: usize,
    pub text: SharedString,
}

#[derive(Clone)]
pub(in crate::orchestrator::view) struct Pull {
    pub checks: Option<CheckState>,
    pub draft: bool,
    pub review: Option<usize>,
}

#[derive(Clone, Default)]
pub(in crate::orchestrator::view) struct Face {
    pub age: SharedString,
    pub agent: Option<Status>,
    pub assignee: Option<Chip>,
    /// Every fact the face shows as a mark, in words, for the tooltip and the
    /// accessible name.
    pub details: SharedString,
    pub labels: Vec<Chip>,
    pub more: Option<SharedString>,
    pub pull: Option<Pull>,
    pub pull_request: bool,
    /// The card's number in its lane's order of implementation.
    pub rank: Option<SharedString>,
}

fn hue(text: &str) -> usize {
    HUES[text.bytes().map(usize::from).sum::<usize>() % HUES.len()]
}

fn clip(text: &str) -> SharedString {
    if text.len() <= MAX_LABEL_BYTES {
        return text.to_owned().into();
    }
    let mut end = MAX_LABEL_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end]).into()
}

fn initials(name: &str) -> SharedString {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .map_or('?', |first| first.to_uppercase().next().unwrap_or(first))
        .to_string()
        .into()
}

/// A run that still holds an agent: a finished one says nothing on the card.
fn running(status: Status) -> bool {
    matches!(
        status,
        Status::Starting | Status::Working | Status::NeedsInput | Status::Idle
    )
}

pub(super) fn prepare(
    item: &Item,
    status: Option<&PrStatus>,
    agent: Option<Status>,
    rank: Option<usize>,
    now: DateTime<Utc>,
) -> Face {
    let pull_request = item.pull_request.is_some() || item.key.native_id.starts_with("pr/");
    let mut details = Vec::new();
    if let Some(rank) = rank {
        details.push(fl!("kanban-rank-detail", number = rank));
    }
    let pull = (pull_request && matches!(item.state.as_str(), "open" | "draft")).then(|| {
        let draft = item.state == "draft";
        details.push(if draft {
            fl!("kanban-card-draft")
        } else {
            fl!("kanban-card-ready")
        });
        let review = pr_cells::review_text(status, None).map(|(text, hue)| {
            details.push(text);
            hue
        });
        let checks = status.and_then(PrStatus::checks_state);
        if let Some(checks) = checks {
            details.push(match checks {
                CheckState::Failed => fl!("kanban-card-checks-failed"),
                CheckState::Pending => fl!("kanban-card-checks-pending"),
                CheckState::Passed => fl!("kanban-card-checks-passed"),
                CheckState::Skipped => fl!("kanban-card-checks-skipped"),
            });
        }
        Pull {
            checks,
            draft,
            review,
        }
    });
    let agent = agent.filter(|status| running(*status));
    if let Some(agent) = agent {
        details.push(fl!("kanban-card-agent", status = agent.label()));
    }
    let assignee = item.assignee.as_deref().filter(|name| !name.is_empty());
    if let Some(name) = assignee {
        details.push(fl!("kanban-card-assignee", name = name.to_owned()));
    }
    if !item.labels.is_empty() {
        details.push(fl!("kanban-card-labels", labels = item.labels.join(", ")));
    }
    let age = item
        .updated_at
        .map(|at| crate::ago::text(crate::ago::since(at.timestamp(), now.timestamp())))
        .unwrap_or_default();
    if !age.is_empty() {
        details.push(fl!("kanban-card-updated", age = age.clone()));
    }
    let hidden = item.labels.len().saturating_sub(MAX_LABELS);
    Face {
        age: age.into(),
        agent,
        assignee: assignee.map(|name| Chip {
            hue: hue(name),
            text: initials(name),
        }),
        details: details.join("; ").into(),
        labels: item
            .labels
            .iter()
            .take(MAX_LABELS)
            .map(|label| Chip {
                hue: hue(label),
                text: clip(label),
            })
            .collect(),
        more: (hidden > 0).then(|| fl!("kanban-card-more", count = hidden).into()),
        pull,
        pull_request,
        rank: rank.map(|rank| rank.to_string().into()),
    }
}
