//! A Forgejo pull request in the shared pull request shape, so the Pull
//! requests list shows it beside GitHub's.

use super::{Listed, issue::Entry};
use crate::{Item, PullRequest, SourceKey};
use serde::Deserialize;

/// One entry of `GET /repos/{owner}/{repo}/pulls`.
#[derive(Deserialize)]
pub struct PullRequestNode {
    #[serde(flatten)]
    entry: Entry,
    /// Absent before Forgejo sent it, which reads as ready.
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    merged: bool,
    head: Branch,
    base: Branch,
    additions: Option<u64>,
    deletions: Option<u64>,
    review_comments: Option<u64>,
}

/// One side of a pull request.
#[derive(Deserialize)]
struct Branch {
    #[serde(rename = "ref")]
    name: String,
    sha: Option<String>,
    /// `null` once a fork the pull request came from is deleted.
    repo: Option<Repository>,
}

#[derive(Deserialize)]
struct Repository {
    /// `owner/name`.
    full_name: String,
}

impl PullRequestNode {
    /// The state word the list filters and colors by.
    fn state(&self) -> &'static str {
        match self.entry.state.as_str() {
            _ if self.merged => "merged",
            "closed" => "closed",
            _ if self.draft => "draft",
            _ => "open",
        }
    }
}

impl Listed for PullRequestNode {
    const COLLECTION: &'static str = "pulls";
    const FILTER: &'static [(&'static str, &'static str)] = &[("state", "open")];

    fn number(&self) -> u64 {
        self.entry.number
    }

    fn into_item(self, source: &SourceKey) -> Option<Item> {
        let number = self.entry.number;
        let state = self.state();
        // A branch of the repository itself is named as the source names
        // it, so it never reads as a fork; a fork keeps Forgejo's name.
        let head_repository = match (self.head.repo, &self.base.repo) {
            (Some(head), Some(base)) if head.full_name == base.full_name => {
                Some(source.repository.clone())
            }
            (head, _) => head.map(|repo| repo.full_name),
        };
        let mut item = self.entry.into_item(source, format!("pr/{number}"));
        item.state = state.to_owned();
        if let Some(activity) = item.activity.as_mut() {
            activity.review_comments = self.review_comments;
        }
        item.pull_request = Some(PullRequest {
            number,
            additions: self.additions,
            deletions: self.deletions,
            base_ref: self.base.name,
            head_ref: self.head.name,
            base_sha: self.base.sha.unwrap_or_default(),
            head_sha: self.head.sha.unwrap_or_default(),
            head_repository,
        });
        Some(item)
    }
}
