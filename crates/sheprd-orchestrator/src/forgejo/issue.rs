//! A Forgejo issue in GitHub's row shape, and what an issue and a pull
//! request both carry.

use super::Listed;
use crate::{
    Activity, Item, ItemKey, SourceKey,
    kanban::{
        facts::{LabelCompleteness, NativeStateProvenance, WorkflowFacts},
        history::Record,
    },
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, de::IgnoredAny};

/// An account as an issue or pull request names it.
#[derive(Deserialize)]
pub(super) struct User {
    login: String,
}

#[derive(Deserialize)]
struct Label {
    name: String,
}

/// The fields Forgejo gives an issue and a pull request alike. Its title
/// and body are the author's, kept as untrusted text, as every source's.
#[derive(Deserialize)]
pub(super) struct Entry {
    pub(super) number: u64,
    title: String,
    body: Option<String>,
    pub(super) state: String,
    html_url: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    pub(super) closed_at: Option<DateTime<Utc>>,
    user: Option<User>,
    /// Forgejo sends `null` rather than an empty list where there are none.
    labels: Option<Vec<Label>>,
    assignee: Option<User>,
    assignees: Option<Vec<User>>,
    comments: Option<u64>,
}

impl Entry {
    pub(super) fn facts(&self, settled_at: Option<DateTime<Utc>>) -> WorkflowFacts {
        WorkflowFacts {
            labels: self.labels.as_ref().map(|_| LabelCompleteness::Complete),
            native_state: Some(NativeStateProvenance::Tracker),
            settled_at,
        }
    }

    /// The row keyed `native_id`, shown `#<number>`: Forgejo numbers issues
    /// and pull requests in one sequence, and writes both that way.
    pub(super) fn into_item(self, source: &SourceKey, native_id: String) -> Item {
        let login = |user: User| Some(user.login).filter(|login| !login.is_empty());
        let assignee = self
            .assignee
            .or_else(|| {
                self.assignees
                    .and_then(|assignees| assignees.into_iter().next())
            })
            .and_then(login);
        Item {
            key: ItemKey {
                source: source.clone(),
                native_id,
            },
            identifier: format!("#{}", self.number),
            title: self.title,
            description: self.body.filter(|body| !body.is_empty()),
            state: self.state.to_ascii_lowercase(),
            url: Some(self.html_url),
            author: self.user.and_then(login),
            assignee,
            labels: self
                .labels
                .unwrap_or_default()
                .into_iter()
                .map(|label| label.name)
                .collect(),
            parent_id: None,
            blocked_by: Vec::new(),
            acceptance: None,
            priority: None,
            created_at: Some(self.created_at),
            updated_at: Some(self.updated_at),
            pull_request: None,
            activity: Some(Activity {
                comments: self.comments,
                ..Activity::default()
            }),
        }
    }
}

/// One entry of `GET /repos/{owner}/{repo}/issues?type=issues`.
#[derive(Deserialize)]
pub struct IssueNode {
    #[serde(flatten)]
    entry: Entry,
    /// Set on a pull request, which `type=issues` leaves out; one listed
    /// anyway is skipped, since the pull request listing has it.
    pull_request: Option<IgnoredAny>,
}

impl IssueNode {
    pub(super) fn into_record(self, source: &SourceKey) -> Option<Record> {
        if self.entry.state != "closed" {
            return None;
        }
        let facts = self.entry.facts(self.entry.closed_at);
        Some(Record {
            facts,
            item: self.into_item(source)?,
        })
    }
}

impl Listed for IssueNode {
    const COLLECTION: &'static str = "issues";
    const FILTER: &'static [(&'static str, &'static str)] =
        &[("state", "open"), ("type", "issues")];

    fn number(&self) -> u64 {
        self.entry.number
    }

    fn into_item(self, source: &SourceKey) -> Option<Item> {
        let number = self.entry.number.to_string();
        self.pull_request
            .is_none()
            .then(|| self.entry.into_item(source, number))
    }
}
