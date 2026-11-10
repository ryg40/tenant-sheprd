//! `orchestrator items|issues|prs|tasks`: what the orchestrator last synced for a
//! repository, or with `--forgejo HOST` for a Forgejo account, read here from
//! its local database, without Sheprd and without the network.
use crate::{
    cli::control::Items,
    control::client::Output,
    orchestrator::{self, Item, Repository},
};
use serde::Serialize;
use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
};

/// One synced issue or pull request, as `--json` prints it.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct ItemInfo {
    pub identifier: String,
    pub title: String,
    pub state: String,
    pub url: Option<String>,
    pub author: Option<String>,
    /// Who a Beads task is assigned to.
    pub assignee: Option<String>,
    pub labels: Vec<String>,
    /// Beads' priority, P0 the most urgent.
    pub priority: Option<i64>,
    /// The tasks a Beads task waits on.
    pub blocked_by: Vec<String>,
    /// The pull request's number, when the item is one.
    pub pull_request: Option<u64>,
}

/// The items `which` asks for, most recently updated first.
pub(super) fn select(mut items: Vec<Item>, which: Items) -> Vec<ItemInfo> {
    items.retain(|item| match which {
        Items::All => true,
        Items::Issues => item.pull_request.is_none(),
        Items::PullRequests => item.pull_request.is_some(),
        Items::Tasks => item.key.source.provider == orchestrator::Provider::Beads,
    });
    items.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.identifier.cmp(&b.identifier))
    });
    items
        .into_iter()
        .map(|item| ItemInfo {
            pull_request: item.pull_request.as_ref().map(|pr| pr.number),
            identifier: item.identifier,
            title: item.title,
            state: item.state,
            url: item.url,
            author: item.author,
            assignee: item.assignee,
            labels: item.labels,
            priority: item.priority,
            blocked_by: item.blocked_by,
        })
        .collect()
}

/// One line per item: its identifier, state, title, author, assignee, and
/// address.
pub(super) fn describe(items: &[ItemInfo]) -> String {
    let mut text = String::new();
    for item in items {
        let _ = write!(text, "{}\t{}\t{}", item.identifier, item.state, item.title);
        if let Some(author) = &item.author {
            let _ = write!(text, "\t@{author}");
        }
        if let Some(assignee) = &item.assignee {
            let _ = write!(text, "\t->{assignee}");
        }
        if let Some(url) = &item.url {
            let _ = write!(text, "\t{url}");
        }
        text.push('\n');
    }
    text.trim_end().to_owned()
}

/// The repository's git common directory, resolved as the orchestrator
/// keys its database.
pub(super) fn git_common_dir(folder: &Path) -> crate::Result<PathBuf> {
    let output = sheprd_client::new_command("git")
        .arg("-C")
        .arg(folder)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .map_err(|source| crate::Error::GitCommonDir {
            path: folder.to_owned(),
            source,
        })?;
    let text = String::from_utf8(output.stdout).ok();
    let common = text
        .as_deref()
        .map(str::trim)
        .filter(|common| output.status.success() && !common.is_empty())
        .ok_or_else(|| crate::Error::NotAGitRepository {
            path: folder.to_owned(),
        })?;
    std::fs::canonicalize(common).map_err(|source| crate::Error::GitCommonDir {
        path: folder.to_owned(),
        source,
    })
}

pub(super) fn list(repo: Option<&Path>, which: Items) -> crate::Result<Output> {
    let folder = match repo {
        Some(repo) => repo.to_owned(),
        None => std::env::current_dir()?,
    };
    let common = git_common_dir(&folder)?;
    let path = orchestrator::database_path(&Repository::Local(common)).ok_or(
        crate::Error::OrchestratorItems(orchestrator::Error::NoDataDirectory),
    )?;
    read(
        &path,
        which,
        "The orchestrator has not synced this repository yet; open it with `orchestrator open`",
        "this repository",
    )
}

/// What the Issues section last synced of every repository the Forgejo
/// token for `host` reads.
pub(super) fn list_account(host: String, which: Items) -> crate::Result<Output> {
    let path = orchestrator::database_path(&Repository::ForgejoAccount { host }).ok_or(
        crate::Error::OrchestratorItems(orchestrator::Error::NoDataDirectory),
    )?;
    read(
        &path,
        which,
        "Sheprd has not synced this Forgejo account yet; it does while FORGEJO_HOST names it and FORGEJO_TOKEN is set in your shell",
        "this Forgejo account",
    )
}

/// The items `which` asks for in the database at `path`, which listing
/// never creates: `unsynced` says why there is none, and `whose` names
/// whose items an empty answer is about.
pub(super) fn read(
    path: &Path,
    which: Items,
    unsynced: &str,
    whose: &str,
) -> crate::Result<Output> {
    // Opening creates a missing database, which listing must not do.
    if !path.is_file() {
        return Ok(Output::data(unsynced, &Vec::<ItemInfo>::new())?.nothing());
    }
    let items = orchestrator::Store::open(path)
        .and_then(|store| store.items())
        .map_err(crate::Error::OrchestratorItems)?;
    let items = select(items, which);
    if items.is_empty() {
        let kind = match which {
            Items::All => "issues or pull requests are",
            Items::Issues => "issues are",
            Items::PullRequests => "pull requests are",
            Items::Tasks => "Beads tasks are",
        };
        return Output::data(format!("No {kind} synced for {whose}"), &items);
    }
    Output::data(describe(&items), &items)
}
