//! A Forgejo repository's listing: its addresses, numbered pages until a
//! short one, the cap, collections a repository turns off, and refusals.
use super::forgejo_nodes::{forgejo, issue_json, pull_json};
use super::*;
use crate::forge::{Credential, Scheme};
use crate::forgejo::{
    self, IssueNode, MAX_OPEN, MAX_PAGES, PAGE, Page, PullRequestNode, Reply, combine, list,
    page_url, status_error,
};
use crate::service::retry_wait;
use secrecy::SecretString;
use std::{collections::HashMap, time::Duration};

/// A repository's open issues as Forgejo pages them: `size` to a page
/// (its `MAX_RESPONSE_ITEMS`), with the count in `X-Total-Count`.
struct Repository {
    open: u64,
    size: usize,
    total: bool,
}

impl Repository {
    fn answer(&self, page: u32) -> Result<Reply<IssueNode>> {
        let start = u64::from(page - 1) * self.size as u64 + 1;
        let nodes = (start..=self.open)
            .take(self.size)
            .map(|number| serde_json::from_value(issue_json(number)).unwrap())
            .collect();
        Ok(Reply::Page(Page {
            nodes,
            total: self.total.then_some(self.open),
        }))
    }
}

fn numbers(items: &[Item]) -> Vec<u64> {
    items
        .iter()
        .map(|item| item.key.native_id.parse().unwrap())
        .collect()
}

#[test]
fn listing_addresses_name_the_repository_and_its_page() {
    let url = page_url::<IssueNode>(&forgejo(), Scheme::Https, 3).unwrap();
    assert_eq!(url.scheme(), "https");
    assert_eq!(url.host_str(), Some("forgejo.example.com"));
    assert_eq!(url.port(), None, "an SSH remote's port is never the API's");
    assert_eq!(url.path(), "/api/v1/repos/team/app/issues");
    let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["state"], "open");
    assert_eq!(query["type"], "issues");
    assert_eq!(query["limit"], "50");
    assert_eq!(query["page"], "3");
    assert_eq!(query["sort"], "oldest");

    let url = page_url::<PullRequestNode>(&forgejo(), Scheme::Https, 1).unwrap();
    assert_eq!(url.path(), "/api/v1/repos/team/app/pulls");
    let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["state"], "open");
    assert_eq!(query["page"], "1");
    assert!(!query.contains_key("type"));

    let served = SourceKey {
        host: "git.example.com:8443".into(),
        repository: "forgejo/team/app".into(),
        ..forgejo()
    };
    let url = page_url::<IssueNode>(&served, Scheme::Http, 1).unwrap();
    assert!(
        url.as_str()
            .starts_with("http://git.example.com:8443/forgejo/api/v1/repos/team/app/issues?"),
        "{url}"
    );
}

#[test]
fn a_repository_path_without_an_owner_and_name_is_refused() {
    for repository in ["app", "team//app", "/app", "team/"] {
        let source = SourceKey {
            repository: repository.into(),
            ..forgejo()
        };
        assert!(
            matches!(
                page_url::<IssueNode>(&source, Scheme::Https, 1),
                Err(forgejo::Error::Address(path)) if path == repository
            ),
            "{repository}"
        );
    }
}

#[test]
fn pages_are_read_until_a_short_one() {
    let repository = Repository {
        open: 120,
        size: PAGE,
        total: false,
    };
    let mut asked = Vec::new();
    let (items, complete) = list(&forgejo(), &mut |page| {
        asked.push(page);
        repository.answer(page)
    })
    .unwrap()
    .unwrap();
    assert!(complete);
    assert_eq!(asked, [1, 2, 3]);
    assert_eq!(numbers(&items), (1..=120).collect::<Vec<_>>());
}

#[test]
fn an_exact_multiple_of_the_page_ends_on_an_empty_page() {
    let repository = Repository {
        open: 100,
        size: PAGE,
        total: false,
    };
    let mut asked = 0;
    let (items, complete) = list(&forgejo(), &mut |page| {
        asked += 1;
        repository.answer(page)
    })
    .unwrap()
    .unwrap();
    assert!(complete);
    assert_eq!((asked, items.len()), (3, 100));
}

/// A Forgejo whose `MAX_RESPONSE_ITEMS` is below the page asked for answers
/// every page short; its count keeps the listing going, so a complete one
/// never drops the issues past the first page.
#[test]
fn a_smaller_server_page_reads_on_while_the_count_says_more() {
    let repository = Repository {
        open: 75,
        size: 20,
        total: true,
    };
    let (items, complete) = list(&forgejo(), &mut |page| repository.answer(page))
        .unwrap()
        .unwrap();
    assert!(complete);
    assert_eq!(numbers(&items), (1..=75).collect::<Vec<_>>());
}

#[test]
fn a_listing_past_the_cap_is_shown_but_not_complete() {
    let repository = Repository {
        open: 5_000,
        size: PAGE,
        total: true,
    };
    let (items, complete) = list(&forgejo(), &mut |page| repository.answer(page))
        .unwrap()
        .unwrap();
    assert!(!complete);
    assert_eq!(items.len(), MAX_OPEN);
}

/// A server that ignores `page` answers the first page again and again;
/// the listing stops rather than asking forever, and is not complete.
#[test]
fn a_page_with_nothing_new_ends_the_listing_incomplete() {
    let repository = Repository {
        open: 500,
        size: PAGE,
        total: false,
    };
    let mut asked = 0;
    let (items, complete) = list(&forgejo(), &mut |_| {
        asked += 1;
        repository.answer(1)
    })
    .unwrap()
    .unwrap();
    assert_eq!(asked, 2);
    assert!(!complete);
    assert_eq!(items.len(), PAGE);
}

/// A server that ignores `type=issues` answers full pages of distinct pull
/// requests, which the issue listing skips: the page bound stops it, and
/// the listing is not complete.
#[test]
fn full_pages_of_skipped_entries_end_at_the_page_bound_incomplete() {
    let mut asked = 0;
    let (items, complete) = list(&forgejo(), &mut |page| {
        asked += 1;
        let start = u64::from(page - 1) * PAGE as u64 + 1;
        let nodes = (start..start + PAGE as u64)
            .map(|number| {
                let mut value = issue_json(number);
                value["pull_request"] = serde_json::json!({"merged": false, "draft": false});
                serde_json::from_value::<IssueNode>(value).unwrap()
            })
            .collect();
        Ok(Reply::Page(Page { nodes, total: None }))
    })
    .unwrap()
    .unwrap();
    assert!(!complete);
    assert!(items.is_empty());
    assert!(asked <= MAX_PAGES);
}

#[test]
fn a_failed_page_fails_the_listing() {
    let repository = Repository {
        open: 120,
        size: PAGE,
        total: false,
    };
    let result = list(&forgejo(), &mut |page| match page {
        1 => repository.answer(page),
        _ => Err(forgejo::Error::RateLimit.into()),
    });
    assert!(matches!(
        result,
        Err(Error::Forgejo(forgejo::Error::RateLimit))
    ));
}

/// A repository with its issues turned off answers their listing 404; its
/// pull requests still list, and the listing is complete.
#[test]
fn a_collection_turned_off_lists_as_empty() {
    let missing = list::<IssueNode>(&forgejo(), &mut |_| Ok(Reply::Missing)).unwrap();
    assert!(missing.is_none());
    let pulls = list::<PullRequestNode>(&forgejo(), &mut |_| {
        let nodes = vec![serde_json::from_value(pull_json(8)).unwrap()];
        Ok(Reply::Page(Page { nodes, total: None }))
    })
    .unwrap();
    let (items, complete) = combine(&forgejo(), Credential::Anonymous, missing, pulls).unwrap();
    assert!(complete);
    assert_eq!(
        items
            .iter()
            .map(|item| item.key.native_id.as_str())
            .collect::<Vec<_>>(),
        ["pr/8"]
    );

    let issues = Some((vec![], false));
    let (_, complete) = combine(&forgejo(), Credential::Anonymous, issues, None).unwrap();
    assert!(!complete, "a capped collection keeps the listing capped");
}

/// Both collections missing means the repository is: hidden from a reader
/// without a token, or not there for one with a token.
#[test]
fn both_collections_missing_is_the_repositorys_refusal() {
    let token = SecretString::from("forgejo-token");
    assert!(matches!(
        combine(&forgejo(), Credential::Anonymous, None, None),
        Err(forgejo::Error::SignIn { repository, host })
            if repository == "team/app" && host == "forgejo.example.com"
    ));
    assert!(matches!(
        combine(&forgejo(), Credential::Withheld, None, None),
        Err(forgejo::Error::InsecureRemote { .. })
    ));
    assert!(matches!(
        combine(&forgejo(), Credential::Token(&token), None, None),
        Err(forgejo::Error::NotFound { repository }) if repository == "team/app"
    ));
}

#[test]
fn refusals_say_whether_a_token_is_needed_or_wrong() {
    let token = SecretString::from("forgejo-token");
    let sent = Credential::Token(&token);
    let source = forgejo();
    for status in [401, 403, 404] {
        assert!(matches!(
            status_error(status, &source, Credential::Anonymous),
            forgejo::Error::SignIn { .. }
        ));
        assert!(matches!(
            status_error(status, &source, Credential::Withheld),
            forgejo::Error::InsecureRemote { .. }
        ));
    }
    assert!(matches!(
        status_error(401, &source, sent),
        forgejo::Error::Authentication
    ));
    assert!(matches!(
        status_error(403, &source, sent),
        forgejo::Error::Forbidden { .. }
    ));
    assert!(matches!(
        status_error(404, &source, sent),
        forgejo::Error::NotFound { .. }
    ));
    assert!(matches!(
        status_error(429, &source, sent),
        forgejo::Error::RateLimit
    ));
    assert!(matches!(
        status_error(502, &source, sent),
        forgejo::Error::Status(502)
    ));
    assert!(matches!(
        Error::from(status_error(500, &source, sent)),
        Error::Forgejo(forgejo::Error::Status(500))
    ));
}

#[test]
fn forgejo_failures_wait_for_the_user_or_the_rate_limit() {
    let half_hour = Duration::from_secs(1800);
    let hour = Duration::from_secs(3600);
    let sign_in = Error::from(forgejo::Error::SignIn {
        repository: "team/app".into(),
        host: "forgejo.example.com".into(),
    });
    assert_eq!(retry_wait(&sign_in, None), half_hour);
    assert_eq!(
        retry_wait(&forgejo::Error::Authentication.into(), None),
        half_hour
    );
    let limited = Error::from(forgejo::Error::RateLimit);
    assert_eq!(retry_wait(&limited, Some(hour)), hour);
    assert_eq!(retry_wait(&limited, None), Duration::from_secs(300));
    assert_eq!(
        retry_wait(&forgejo::Error::Status(500).into(), None),
        Duration::from_secs(300)
    );
}

/// Cancelling stops a sync before it sends a request.
#[test]
fn a_cancelled_sync_sends_nothing() {
    let token = SecretString::from("forgejo-token");
    let mut cooldown = None;
    let result = forgejo::sync(
        (&forgejo(), Scheme::Https),
        Some(&token),
        None,
        &|| true,
        &mut cooldown,
    );
    assert!(matches!(result, Err(Error::Cancelled)));
    assert_eq!(cooldown, None);
}
