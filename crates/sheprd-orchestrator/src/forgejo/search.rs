//! Every open issue a token reads on a Forgejo host, across all of its
//! repositories, through `GET /api/v1/repos/issues/search`: the account
//! listing the Issues section shows beside the checkouts. Each issue is
//! keyed by its own repository (`owner/name`, from the reply), so it is the
//! same row a checkout of that repository lists. Pull requests are left out
//! (`type=issues`). The listing is stored under the account's own source
//! ([`account_source`]) and replaced only when it is complete.

pub use super::MAX_PAGES;
use super::{Error, MAX_OPEN, PAGE, Reply, TIMEOUT, fetch, issue::Entry};
use crate::{
    Checkpoint, Item, Provider, SourceKey,
    forge::{Credential, Listing, Scheme, agent},
    kanban::history::Record,
};
use chrono::Utc;
use secrecy::SecretString;
use serde::{Deserialize, de::IgnoredAny};
use std::{collections::HashSet, time::Duration};

/// The repository an entry belongs to.
#[derive(Deserialize)]
struct Repository {
    /// `owner/name`.
    full_name: String,
}

/// One entry of the issue search.
#[derive(Deserialize)]
pub struct SearchNode {
    #[serde(flatten)]
    entry: Entry,
    repository: Option<Repository>,
    /// Set on a pull request, which `type=issues` leaves out; one listed
    /// anyway is skipped.
    pull_request: Option<IgnoredAny>,
}

impl SearchNode {
    pub(super) fn into_record(self, host: &str) -> Option<Record> {
        if self.entry.state != "closed" {
            return None;
        }
        let facts = self.entry.facts(self.entry.closed_at);
        Some(Record {
            facts,
            item: self.into_item(host)?,
        })
    }

    /// What tells entries apart across repositories: the repository and the
    /// number, which repositories reuse. `None` without a repository.
    fn key(&self) -> Option<(String, u64)> {
        let repository = self.repository.as_ref()?;
        Some((repository.full_name.to_ascii_lowercase(), self.entry.number))
    }

    /// The issue, keyed by its own repository on `host`; `None` for a pull
    /// request, or an entry naming no `owner/name` repository.
    pub fn into_item(self, host: &str) -> Option<Item> {
        if self.pull_request.is_some() {
            return None;
        }
        let repository = self.repository?.full_name;
        let (owner, name) = repository.split_once('/')?;
        if owner.is_empty() || name.is_empty() || name.contains('/') {
            return None;
        }
        let source = SourceKey {
            provider: Provider::Forgejo,
            host: host.to_owned(),
            repository,
        };
        let number = self.entry.number.to_string();
        Some(self.entry.into_item(&source, number))
    }
}

/// The source an account listing of `host` is stored and shown under. Its
/// repository is the host, which holds no `/`, so it is never a
/// repository's own source.
pub fn account_source(host: &str) -> SourceKey {
    SourceKey {
        provider: Provider::Forgejo,
        host: host.to_owned(),
        repository: host.to_owned(),
    }
}

/// The API address of page `number` of the open issues `host` shows,
/// oldest first. Forgejo's default, `latest`, puts each issue opened during
/// the listing on page one and moves the rest a page on; `oldest` adds it
/// at the end. The token goes there, so `host` must be a host and port
/// alone.
pub fn search_url(host: &str, number: u32) -> Result<url::Url, Error> {
    let invalid = || Error::Address(host.to_owned());
    let mut url = url::Url::parse(&format!("https://{host}")).map_err(|_| invalid())?;
    let bare = url.host_str().is_some_and(|name| !name.is_empty())
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none();
    if !bare {
        return Err(invalid());
    }
    url.path_segments_mut()
        .map_err(|()| invalid())?
        .pop_if_empty()
        .extend(["api", "v1", "repos", "issues", "search"]);
    url.query_pairs_mut()
        .append_pair("type", "issues")
        .append_pair("state", "open")
        .append_pair("sort", "oldest")
        .append_pair("limit", &PAGE.to_string())
        .append_pair("page", &number.to_string());
    Ok(url)
}

/// Every open issue `fetch` pages through for `host`, and whether the
/// listing is complete. A page shorter than [`PAGE`] is the last unless the
/// count Forgejo gave says more are open. It is capped past [`MAX_OPEN`]
/// issues or [`MAX_PAGES`] pages, or when a page adds nothing new, which
/// would otherwise be asked for again and again. An issue moving between
/// pages while they are read is listed once; one moved onto a page already
/// read is listed by the next sync.
pub fn list(
    host: &str,
    fetch: &mut impl FnMut(u32) -> crate::Result<Reply<SearchNode>>,
) -> crate::Result<(Vec<Item>, bool)> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    for number in 1..=MAX_PAGES {
        // The search is the instance's, never a repository's turned off: a
        // 404 means this Forgejo has no such endpoint.
        let Reply::Page(page) = fetch(number)? else {
            return Err(Error::Status(404).into());
        };
        let read = page.nodes.len();
        let before = seen.len();
        for node in page.nodes {
            if node.key().is_some_and(|key| seen.insert(key)) {
                items.extend(node.into_item(host));
            }
        }
        let more = read >= PAGE || page.total.is_some_and(|total| (seen.len() as u64) < total);
        if read == 0 || !more {
            return Ok((items, true));
        }
        if items.len() >= MAX_OPEN || seen.len() == before {
            return Ok((items, false));
        }
    }
    Ok((items, false))
}

/// Lists the open issues `token` reads on `host`, over HTTPS alone.
/// Blocks on the network: call it from a worker.
pub fn sync(
    host: &str,
    token: &SecretString,
    cancelled: &impl Fn() -> bool,
    cooldown: &mut Option<Duration>,
) -> crate::Result<Listing> {
    let started = Utc::now();
    let agent = agent(TIMEOUT);
    let source = account_source(host);
    let credential = Credential::Token(token);
    let (items, complete) = list(host, &mut |number| {
        if cancelled() {
            return Err(crate::Error::Cancelled);
        }
        let url = search_url(host, number)?;
        // A refusal names the host, which stands in for a repository here.
        Ok(fetch(
            &agent,
            &url,
            (&source, Scheme::Https),
            credential,
            "issues/search",
            cooldown,
        )?)
    })?;
    Ok(Listing {
        items,
        checkpoint: Checkpoint {
            updated_at: Some(started),
            last_full_at: Some(Utc::now()),
            ..Checkpoint::default()
        },
        complete,
    })
}
