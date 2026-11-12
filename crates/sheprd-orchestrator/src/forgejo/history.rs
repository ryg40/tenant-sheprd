//! Recent settled observations. A bounded sample never replaces an active listing.

use super::{IssueNode, Listed, PAGE, PullRequestNode, Reply, SearchNode, TIMEOUT, fetch};
use crate::{
    Result, SourceKey,
    forge::{Credential, Scheme, agent},
    kanban::{
        Error,
        history::{MAX_ACCOUNT_RECORDS, MAX_RECORDS, Read, Record, WINDOW, identity},
    },
};
use chrono::{DateTime, Utc};
use secrecy::SecretString;
use std::{collections::HashSet, time::Duration};

/// Shared by both repository feeds, or by the account's one feed.
pub const MAX_PAGES: u32 = 10;

/// Read issues and pulls separately, retaining their native closed/merged facts.
/// Blocks on the network; callers supply the existing worker's cancellation and backoff.
pub fn repository(
    (source, scheme): (&SourceKey, Scheme),
    token: Option<&SecretString>,
    cutoff: DateTime<Utc>,
    cancelled: &impl Fn() -> bool,
    cooldown: &mut Option<Duration>,
) -> Result<Read> {
    let agent = agent(TIMEOUT);
    let credential = Credential::new(scheme, token);
    let mut issue_cooldown = None;
    let mut pull_cooldown = None;
    let result = list_repository(
        source,
        cutoff,
        &mut |page| {
            if cancelled() {
                return Err(crate::Error::Cancelled);
            }
            let url = repository_url::<IssueNode>(source, scheme, cutoff, page)?;
            Ok(fetch(
                &agent,
                &url,
                (source, scheme),
                credential,
                "issues",
                &mut issue_cooldown,
            )?)
        },
        &mut |page| {
            if cancelled() {
                return Err(crate::Error::Cancelled);
            }
            let url = repository_url::<PullRequestNode>(source, scheme, cutoff, page)?;
            Ok(fetch(
                &agent,
                &url,
                (source, scheme),
                credential,
                "pulls",
                &mut pull_cooldown,
            )?)
        },
    );
    *cooldown = issue_cooldown.or(pull_cooldown);
    result
}

/// A repository sample with injectable readers, sharing one history-cycle page budget.
pub fn list_repository(
    source: &SourceKey,
    cutoff: DateTime<Utc>,
    issues: &mut impl FnMut(u32) -> Result<Reply<IssueNode>>,
    pulls: &mut impl FnMut(u32) -> Result<Reply<PullRequestNode>>,
) -> Result<Read> {
    let mut remaining = MAX_PAGES;
    let mut issues = list(
        cutoff,
        MAX_RECORDS,
        &mut remaining,
        |node: IssueNode| {
            node.into_record(source)
                .filter(|record| record.item.updated_at >= Some(cutoff))
        },
        issues,
    )?;
    let pulls = list(
        cutoff,
        MAX_RECORDS,
        &mut remaining,
        |node: PullRequestNode| node.into_record(source),
        pulls,
    )?;
    issues.records.extend(pulls.records);
    issues.truncated |= pulls.truncated;
    // Sample::from_read retains the newest MAX_RECORDS across both feeds.
    Ok(issues)
}

/// Account history uses only the existing host-wide search, never repository fan-out.
pub fn account(
    host: &str,
    token: &SecretString,
    cutoff: DateTime<Utc>,
    cancelled: &impl Fn() -> bool,
    cooldown: &mut Option<Duration>,
) -> Result<Read> {
    let agent = agent(TIMEOUT);
    let source = super::search::account_source(host);
    list_account(host, cutoff, &mut |page| {
        if cancelled() {
            return Err(crate::Error::Cancelled);
        }
        let url = account_url(host, cutoff, page)?;
        Ok(fetch(
            &agent,
            &url,
            (&source, Scheme::Https),
            Credential::Token(token),
            "issues/search",
            cooldown,
        )?)
    })
}

/// The account's bounded sample with an injectable page reader, as in `search::list`.
pub fn list_account(
    host: &str,
    cutoff: DateTime<Utc>,
    fetch: &mut impl FnMut(u32) -> Result<Reply<SearchNode>>,
) -> Result<Read> {
    let mut remaining = MAX_PAGES;
    list(
        cutoff,
        MAX_ACCOUNT_RECORDS,
        &mut remaining,
        |node: SearchNode| {
            node.into_record(host)
                .filter(|record| record.item.updated_at >= Some(cutoff))
        },
        fetch,
    )
}

pub fn repository_url<N: Listed>(
    source: &SourceKey,
    scheme: Scheme,
    cutoff: DateTime<Utc>,
    page: u32,
) -> std::result::Result<url::Url, super::Error> {
    let mut url = super::collection_url(source, scheme, N::COLLECTION)?;
    if N::COLLECTION == IssueNode::COLLECTION {
        url.query_pairs_mut().append_pair("type", "issues");
        url.query_pairs_mut()
            .append_pair("since", &cutoff.to_rfc3339());
    }
    history_query(&mut url, page);
    Ok(url)
}

pub fn account_url(
    host: &str,
    cutoff: DateTime<Utc>,
    page: u32,
) -> std::result::Result<url::Url, super::Error> {
    // Reuse the account's strict bare-host validation.
    let mut url = super::search::search_url(host, page)?;
    url.set_query(None);
    url.query_pairs_mut()
        .append_pair("type", "issues")
        .append_pair("since", &cutoff.to_rfc3339());
    history_query(&mut url, page);
    Ok(url)
}

fn history_query(url: &mut url::Url, page: u32) {
    url.query_pairs_mut()
        .append_pair("state", "closed")
        .append_pair("sort", "recentupdate")
        .append_pair("limit", &PAGE.to_string())
        .append_pair("page", &page.to_string());
}

/// An intentional record cap is a usable partial sample. A missing page, repeated
/// identity, ignored state/type filter, or broken ordering is not a replaceable sample.
fn list<N>(
    cutoff: DateTime<Utc>,
    limit: usize,
    remaining: &mut u32,
    convert: impl Fn(N) -> Option<Record>,
    fetch: &mut impl FnMut(u32) -> Result<Reply<N>>,
) -> Result<Read> {
    let mut records = Vec::new();
    let mut seen = HashSet::new();
    let mut previous = None;
    let mut total = None;
    let mut page = 0;
    let mut page_size = PAGE;
    while *remaining > 0 {
        page += 1;
        *remaining -= 1;
        let Reply::Page(reply) = fetch(page)? else {
            // A history 404 cannot prove the old sample is now empty.
            return Err(Error::HistoryIncomplete.into());
        };
        if reply.nodes.len() > page_size || (page > 1 && reply.total != total) {
            return Err(Error::HistoryIncomplete.into());
        }
        if page == 1 {
            page_size = reply.nodes.len();
        }
        total = reply.total;
        let empty = reply.nodes.is_empty();
        let mut outside_window = false;
        for node in reply.nodes {
            let record = convert(node).ok_or(Error::HistoryIncomplete)?;
            let updated = record.item.updated_at.ok_or(Error::HistoryRecord)?;
            if previous.is_some_and(|last| updated > last) || !seen.insert(identity(&record.item)) {
                return Err(Error::HistoryIncomplete.into());
            }
            previous = Some(updated);
            if updated >= cutoff {
                records.push(record);
            } else {
                outside_window = true;
            }
        }
        let read = seen.len();
        if total.is_some_and(|total| read as u64 > total || (empty && (read as u64) < total)) {
            return Err(Error::HistoryIncomplete.into());
        }
        // A small configured page is not exhaustion. With no count, require an
        // empty page or an ordered cutoff crossing, never merely a short page.
        let exhausted = empty || total == Some(read as u64) || outside_window;
        // Keep the requested limit stable: changing it mid-read changes page offsets.
        // Stop before another configured-size page could exceed the admission cap.
        let capped = read.saturating_add(page_size) > limit;
        if capped || exhausted {
            let truncated = capped && !exhausted;
            return Ok(Read {
                complete: true,
                cutoff,
                records,
                truncated,
            });
        }
    }
    Err(Error::HistoryIncomplete.into())
}

pub(crate) fn cutoff(now: DateTime<Utc>) -> DateTime<Utc> {
    now - WINDOW
}
