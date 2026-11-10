//! A Forgejo account's open issues across its repositories, through the
//! issue search: its address, each entry keyed by its own repository, pages
//! until a short one, the caps, and when there is an account at all.
use super::*;
use crate::forgejo::{
    MAX_OPEN, PAGE, Page, Reply, SearchNode, account_host_with,
    search::{MAX_PAGES, account_source, list, search_url},
};
use std::{collections::HashMap, ffi::OsString};

const HOST: &str = "forgejo.example.com";

/// An open issue as `GET /repos/issues/search` lists it: an issue's fields
/// and the repository it belongs to.
fn search_json(repository: &str, number: u64) -> serde_json::Value {
    let (owner, name) = repository.split_once('/').unwrap();
    serde_json::json!({
        "id": 7000 + number,
        "url": format!("https://{HOST}/api/v1/repos/{repository}/issues/{number}"),
        "html_url": format!("https://{HOST}/{repository}/issues/{number}"),
        "number": number,
        "user": {"id": 3, "login": "alice", "full_name": "", "username": "alice"},
        "original_author": "",
        "original_author_id": 0,
        "title": format!("Issue {number} of {repository}"),
        "body": "Steps to reproduce",
        "ref": "",
        "assets": [],
        "labels": [{"id": 1, "name": "bug", "color": "ee0701", "description": "", "exclusive": false}],
        "milestone": null,
        "assignee": null,
        "assignees": [{"id": 4, "login": "bob", "username": "bob"}],
        "state": "open",
        "is_locked": false,
        "comments": 2,
        "created_at": "2026-10-08T23:23:37+02:00",
        "updated_at": "2026-10-09T01:00:00Z",
        "closed_at": null,
        "due_date": null,
        "pull_request": null,
        "repository": {"id": 7, "name": name, "owner": owner, "full_name": repository},
        "pin_order": 0,
    })
}

fn node(value: serde_json::Value) -> SearchNode {
    serde_json::from_value(value).unwrap()
}

/// `open` issues spread over `repositories` in turn, numbered so two
/// repositories hold the same numbers, `size` to a page, with the count in
/// `X-Total-Count` when `total` is set.
struct Account {
    repositories: &'static [&'static str],
    open: u64,
    size: usize,
    total: bool,
}

impl Account {
    fn answer(&self, page: u32) -> Result<Reply<SearchNode>> {
        let start = u64::from(page - 1) * self.size as u64;
        let count = self.repositories.len() as u64;
        let nodes = (start..self.open)
            .take(self.size)
            .map(|at| {
                let repository = self.repositories[(at % count) as usize];
                node(search_json(repository, at / count + 1))
            })
            .collect();
        Ok(Reply::Page(Page {
            nodes,
            total: self.total.then_some(self.open),
        }))
    }
}

#[test]
fn the_search_reads_open_issues_of_every_repository_at_the_hosts_root() {
    let url = search_url(HOST, 3).unwrap();
    assert_eq!(url.scheme(), "https");
    assert_eq!(url.host_str(), Some(HOST));
    assert_eq!(url.path(), "/api/v1/repos/issues/search");
    let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["type"], "issues");
    assert_eq!(query["state"], "open");
    assert_eq!(query["sort"], "oldest");
    assert_eq!(query["limit"], PAGE.to_string());
    assert_eq!(query["page"], "3");
    assert_eq!(
        search_url("git.example.com:8443", 1).unwrap().port(),
        Some(8443)
    );
}

/// The token goes to the search's address, so a host that would carry it
/// anywhere but the host's own root is refused.
#[test]
fn a_host_with_more_than_a_host_and_port_is_refused() {
    for host in [
        "",
        "user@forgejo.example.com",
        "forgejo.example.com/git",
        "forgejo.example.com?x=1",
        "forgejo.example.com#x",
    ] {
        assert!(
            matches!(
                search_url(host, 1),
                Err(crate::forgejo::Error::Address(named)) if named == host
            ),
            "{host:?}"
        );
    }
}

#[test]
fn an_entry_is_keyed_by_its_own_repository() {
    let item = node(search_json("team/web", 12)).into_item(HOST).unwrap();
    assert_eq!(
        item.key.source,
        SourceKey {
            provider: Provider::Forgejo,
            host: HOST.into(),
            repository: "team/web".into(),
        }
    );
    assert_eq!(item.key.native_id, "12");
    assert_eq!(item.identifier, "#12");
    assert_eq!(item.title, "Issue 12 of team/web");
    assert_eq!(item.description.as_deref(), Some("Steps to reproduce"));
    assert_eq!(
        item.url.as_deref(),
        Some("https://forgejo.example.com/team/web/issues/12")
    );
    assert_eq!(item.author.as_deref(), Some("alice"));
    assert_eq!(item.assignee.as_deref(), Some("bob"));
    assert_eq!(item.labels, ["bug"]);
    assert_eq!(item.state, "open");
    assert!(item.pull_request.is_none());
    // The same row a checkout of `team/web` lists.
    let checkout = SourceKey {
        provider: Provider::Forgejo,
        host: HOST.into(),
        repository: "team/web".into(),
    };
    assert_eq!(
        item.key.canonical(),
        ItemKey {
            source: checkout,
            native_id: "12".into()
        }
        .canonical()
    );
}

#[test]
fn pull_requests_and_entries_without_a_repository_are_left_out() {
    let mut pull = search_json("team/web", 3);
    pull["pull_request"] = serde_json::json!({"merged": false, "draft": false});
    assert!(node(pull).into_item(HOST).is_none());
    for repository in [
        serde_json::Value::Null,
        serde_json::json!({"id": 1, "name": "web", "owner": "team", "full_name": "web"}),
        serde_json::json!({"id": 1, "name": "", "owner": "team", "full_name": "team/"}),
        serde_json::json!({"id": 1, "name": "web", "owner": "", "full_name": "/web"}),
        serde_json::json!({"id": 1, "name": "x", "owner": "a", "full_name": "a/b/c"}),
    ] {
        let mut entry = search_json("team/web", 4);
        entry["repository"] = repository.clone();
        assert!(node(entry).into_item(HOST).is_none(), "{repository}");
    }
}

#[test]
fn pages_are_read_until_a_short_one_and_numbers_repeat_across_repositories() {
    let account = Account {
        repositories: &["team/app", "team/web"],
        open: 120,
        size: PAGE,
        total: true,
    };
    let mut asked = Vec::new();
    let (items, complete) = list(HOST, &mut |page| {
        asked.push(page);
        account.answer(page)
    })
    .unwrap();
    assert!(complete);
    assert_eq!(asked, [1, 2, 3]);
    assert_eq!(items.len(), 120);
    let keys: std::collections::HashSet<String> =
        items.iter().map(|item| item.key.canonical()).collect();
    assert_eq!(keys.len(), 120, "issue 1 of each repository is its own row");
    assert!(
        items
            .iter()
            .all(|item| item.key.source.host == HOST && item.key.source.repository.contains('/'))
    );
}

#[test]
fn a_short_page_with_more_counted_reads_on() {
    let account = Account {
        repositories: &["team/app"],
        open: 45,
        size: 20,
        total: true,
    };
    let (items, complete) = list(HOST, &mut |page| account.answer(page)).unwrap();
    assert!(complete);
    assert_eq!(items.len(), 45);
}

#[test]
fn a_listing_past_the_cap_is_capped() {
    let account = Account {
        repositories: &["team/app", "team/web"],
        open: MAX_OPEN as u64 + 2 * PAGE as u64,
        size: PAGE,
        total: true,
    };
    let (items, complete) = list(HOST, &mut |page| account.answer(page)).unwrap();
    assert!(!complete);
    assert!(items.len() >= MAX_OPEN);
}

#[test]
fn a_listing_past_the_page_bound_is_capped() {
    // A page of one, counted as more: only the page bound stops it.
    let account = Account {
        repositories: &["team/app"],
        open: u64::from(MAX_PAGES) * 3,
        size: 1,
        total: true,
    };
    let mut asked = 0;
    let (items, complete) = list(HOST, &mut |page| {
        asked += 1;
        account.answer(page)
    })
    .unwrap();
    assert!(!complete);
    assert!(asked <= MAX_PAGES);
    assert!(!items.is_empty());
}

#[test]
fn a_page_with_nothing_new_is_not_asked_for_again() {
    let account = Account {
        repositories: &["team/app"],
        open: 50,
        size: PAGE,
        total: false,
    };
    let mut asked = 0;
    // Every page answers the first again.
    let (items, complete) = list(HOST, &mut |_| {
        asked += 1;
        account.answer(1)
    })
    .unwrap();
    assert!(!complete);
    assert_eq!(asked, 2);
    assert_eq!(items.len(), 50);
}

#[test]
fn an_instance_without_the_search_is_a_failure_not_an_empty_listing() {
    let error = list(HOST, &mut |_| Ok(Reply::Missing)).unwrap_err();
    assert!(
        matches!(error, Error::Forgejo(crate::forgejo::Error::Status(404))),
        "{error:?}"
    );
}

#[test]
fn the_account_source_is_never_a_repositorys() {
    let source = account_source(HOST);
    assert_eq!(source.provider, Provider::Forgejo);
    assert_eq!(source.host, HOST);
    assert!(!source.repository.contains('/'));
}

fn environment(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let pairs: HashMap<String, OsString> = pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), OsString::from(value)))
        .collect();
    move |name| pairs.get(name).cloned()
}

#[test]
fn there_is_an_account_only_with_a_host_and_a_token_for_it() {
    assert_eq!(account_host_with(environment(&[])), None);
    assert_eq!(
        account_host_with(environment(&[("FORGEJO_TOKEN", "abc123")])),
        None,
        "no FORGEJO_HOST, so no host gets the token"
    );
    assert_eq!(
        account_host_with(environment(&[("FORGEJO_HOST", HOST)])),
        None,
        "no token"
    );
    assert_eq!(
        account_host_with(environment(&[
            ("FORGEJO_HOST", HOST),
            ("FORGEJO_TOKEN", " "),
        ])),
        None,
        "a blank token is none"
    );
    assert_eq!(
        account_host_with(environment(&[
            ("FORGEJO_HOST", "https://Forgejo.Example.com/"),
            ("FORGEJO_TOKEN", "abc123"),
        ]))
        .as_deref(),
        Some(HOST)
    );
}

#[test]
fn an_accounts_database_is_keyed_apart_from_every_checkout() {
    let root = std::path::Path::new("/state");
    let account = Repository::ForgejoAccount { host: HOST.into() };
    let path = crate::location::database_path_in(root, &account);
    assert!(path.starts_with(root.join("repositories")));
    assert_ne!(
        path,
        crate::location::database_path_in(root, &Repository::Local(HOST.into()))
    );
    assert_ne!(
        path,
        crate::location::database_path_in(
            root,
            &Repository::ForgejoAccount {
                host: "other.example.com".into()
            }
        )
    );
}
