//! Forgejo's issue and pull request listings, in the shape its API
//! documents (`Issue`, `PullRequest`), read into the shared rows.
use super::*;
use crate::forgejo::{IssueNode, Listed, PullRequestNode};

pub(super) fn forgejo() -> SourceKey {
    SourceKey {
        provider: Provider::Forgejo,
        host: "forgejo.example.com".into(),
        repository: "team/app".into(),
    }
}

fn user(login: &str) -> serde_json::Value {
    serde_json::json!({
        "id": 3, "login": login, "full_name": "", "email": "user@noreply.example.com",
        "avatar_url": "https://forgejo.example.com/avatars/3", "username": login,
    })
}

/// An open issue as `GET /repos/team/app/issues` lists it.
pub(super) fn issue_json(number: u64) -> serde_json::Value {
    serde_json::json!({
        "id": 900 + number,
        "url": format!("https://forgejo.example.com/api/v1/repos/team/app/issues/{number}"),
        "html_url": format!("https://forgejo.example.com/team/app/issues/{number}"),
        "number": number,
        "user": user("alice"),
        "original_author": "",
        "original_author_id": 0,
        "title": format!("Issue {number}"),
        "body": "Steps to reproduce",
        "ref": "",
        "assets": [],
        "labels": [{"id": 1, "name": "bug", "color": "ee0701", "description": "", "exclusive": false}],
        "milestone": null,
        "assignee": null,
        "assignees": null,
        "state": "open",
        "is_locked": false,
        "comments": 4,
        "created_at": "2026-10-08T23:23:37+02:00",
        "updated_at": "2026-10-09T01:00:00Z",
        "closed_at": null,
        "due_date": null,
        "pull_request": null,
        "repository": {"id": 7, "name": "app", "owner": "team", "full_name": "team/app"},
        "pin_order": 0,
    })
}

fn repository(full_name: &str) -> serde_json::Value {
    let (owner, name) = full_name.split_once('/').unwrap();
    serde_json::json!({
        "id": 7, "name": name, "full_name": full_name,
        "owner": user(owner), "private": false, "fork": false,
        "html_url": format!("https://forgejo.example.com/{full_name}"),
    })
}

/// An open pull request as `GET /repos/team/app/pulls` lists it.
pub(super) fn pull_json(number: u64) -> serde_json::Value {
    serde_json::json!({
        "id": 500 + number,
        "url": format!("https://forgejo.example.com/team/app/pulls/{number}"),
        "number": number,
        "user": user("bob"),
        "title": format!("Pull {number}"),
        "body": "",
        "labels": [],
        "milestone": null,
        "assignee": user("carol"),
        "assignees": [user("carol"), user("dave")],
        "requested_reviewers": null,
        "requested_reviewers_teams": null,
        "state": "open",
        "draft": false,
        "is_locked": false,
        "comments": 1,
        "review_comments": 3,
        "additions": 12,
        "deletions": 4,
        "changed_files": 2,
        "html_url": format!("https://forgejo.example.com/team/app/pulls/{number}"),
        "diff_url": format!("https://forgejo.example.com/team/app/pulls/{number}.diff"),
        "patch_url": format!("https://forgejo.example.com/team/app/pulls/{number}.patch"),
        "mergeable": true,
        "merged": false,
        "merged_at": null,
        "merge_commit_sha": null,
        "merged_by": null,
        "allow_maintainer_edit": false,
        "base": {"label": "main", "ref": "main", "sha": "aaa111", "repo_id": 7, "repo": repository("team/app")},
        "head": {"label": "feature", "ref": "feature", "sha": "bbb222", "repo_id": 7, "repo": repository("team/app")},
        "merge_base": "aaa000",
        "due_date": null,
        "created_at": "2026-10-07T10:00:00Z",
        "updated_at": "2026-10-08T10:00:00Z",
        "closed_at": null,
        "pin_order": 0,
        "flow": 0,
    })
}

fn issue(value: serde_json::Value) -> Option<Item> {
    serde_json::from_value::<IssueNode>(value)
        .unwrap()
        .into_item(&forgejo())
}

fn pull(value: serde_json::Value) -> Item {
    serde_json::from_value::<PullRequestNode>(value)
        .unwrap()
        .into_item(&forgejo())
        .unwrap()
}

#[test]
fn an_issue_reads_as_a_numbered_row() {
    let item = issue(issue_json(42)).unwrap();
    assert_eq!(item.key.native_id, "42");
    assert_eq!(
        item.key.canonical(),
        "forgejo:forgejo.example.com:team/app:42"
    );
    assert_eq!(item.identifier, "#42");
    assert_eq!(item.title, "Issue 42");
    assert_eq!(item.description.as_deref(), Some("Steps to reproduce"));
    assert_eq!(item.state, "open");
    assert_eq!(
        item.url.as_deref(),
        Some("https://forgejo.example.com/team/app/issues/42"),
        "the page a person opens, not the API's address"
    );
    assert_eq!(item.author.as_deref(), Some("alice"));
    assert_eq!(item.labels, ["bug"]);
    assert_eq!(item.assignee, None);
    assert_eq!(item.created_at, Some(at("2026-10-08T21:23:37+00:00")));
    assert_eq!(item.updated_at, Some(at("2026-10-09T01:00:00+00:00")));
    assert_eq!(
        item.activity.and_then(|activity| activity.comments),
        Some(4)
    );
    assert!(item.pull_request.is_none());
}

#[test]
fn an_issue_without_optional_fields_still_decodes() {
    let item = issue(serde_json::json!({
        "number": 1, "title": "Bare", "state": "closed",
        "html_url": "https://forgejo.example.com/team/app/issues/1",
        "created_at": "2026-10-08T23:23:37Z", "updated_at": "2026-10-08T23:23:37Z",
        "user": null, "labels": null, "body": "",
    }))
    .unwrap();
    assert_eq!(item.state, "closed");
    assert!(item.description.is_none() && item.author.is_none() && item.labels.is_empty());
    assert!(item.assignee.is_none());
}

/// A pull request the issue listing returns anyway is left to the pull
/// request listing, so it is never listed twice.
#[test]
fn a_pull_request_in_the_issue_listing_is_skipped() {
    let mut value = issue_json(9);
    value["pull_request"] = serde_json::json!({"merged": false, "merged_at": null, "draft": false});
    assert!(issue(value).is_none());
}

#[test]
fn a_pull_request_reads_as_the_shared_pull_request_row() {
    let item = pull(pull_json(7));
    assert_eq!(item.key.native_id, "pr/7");
    assert_eq!(
        item.key.canonical(),
        "forgejo:forgejo.example.com:team/app:pr/7"
    );
    assert_eq!(item.identifier, "#7", "Forgejo writes a pull request #N");
    assert_eq!(item.state, "open");
    assert_eq!(item.author.as_deref(), Some("bob"));
    assert_eq!(item.assignee.as_deref(), Some("carol"));
    assert!(
        item.description.is_none(),
        "an empty body is no description"
    );
    assert_eq!(
        item.url.as_deref(),
        Some("https://forgejo.example.com/team/app/pulls/7")
    );
    let activity = item.activity.unwrap();
    assert_eq!(
        (activity.comments, activity.review_comments),
        (Some(1), Some(3))
    );
    let pr = item.pull_request.as_ref().unwrap();
    assert_eq!(pr.number, 7);
    assert_eq!(
        (pr.base_ref.as_str(), pr.head_ref.as_str()),
        ("main", "feature")
    );
    assert_eq!(
        (pr.base_sha.as_str(), pr.head_sha.as_str()),
        ("aaa111", "bbb222")
    );
    assert_eq!((pr.additions, pr.deletions), (Some(12), Some(4)));
    assert_eq!(
        pr.head_repository.as_deref(),
        Some("team/app"),
        "a branch of the repository itself is no fork"
    );
    assert!(
        item.github_pull_request().is_none(),
        "GitHub's pages and actions stay GitHub's"
    );
}

#[test]
fn a_pull_requests_state_reads_draft_merged_or_closed() {
    let state = |change: &dyn Fn(&mut serde_json::Value)| {
        let mut value = pull_json(3);
        change(&mut value);
        pull(value).state
    };
    assert_eq!(state(&|_| {}), "open");
    assert_eq!(state(&|value| value["draft"] = true.into()), "draft");
    assert_eq!(
        state(&|value| {
            value.as_object_mut().unwrap().remove("draft");
        }),
        "open",
        "a Forgejo that sends no draft field reads as ready"
    );
    assert_eq!(
        state(&|value| {
            value["state"] = "closed".into();
            value["merged"] = true.into();
        }),
        "merged"
    );
    assert_eq!(state(&|value| value["state"] = "closed".into()), "closed");
}

#[test]
fn a_fork_keeps_its_name_and_a_deleted_one_none() {
    let mut value = pull_json(5);
    value["head"]["repo"] = repository("someone/app");
    let pr = pull(value).pull_request.unwrap();
    assert_eq!(pr.head_repository.as_deref(), Some("someone/app"));

    let mut value = pull_json(5);
    value["head"]["repo"] = serde_json::Value::Null;
    value["head"]["sha"] = serde_json::Value::Null;
    value["assignee"] = serde_json::Value::Null;
    let item = pull(value);
    assert_eq!(
        item.assignee.as_deref(),
        Some("carol"),
        "the first of assignees"
    );
    let pr = item.pull_request.unwrap();
    assert_eq!(pr.head_repository, None);
    assert_eq!(pr.head_sha, "");
}

#[test]
fn a_listing_reads_as_a_json_array() {
    let page = serde_json::json!([issue_json(1), issue_json(2)]);
    let nodes: Vec<IssueNode> = serde_json::from_value(page).unwrap();
    assert_eq!(nodes.iter().map(Listed::number).collect::<Vec<_>>(), [1, 2]);
    let page = serde_json::json!([pull_json(3)]);
    let nodes: Vec<PullRequestNode> = serde_json::from_value(page).unwrap();
    assert_eq!(nodes.len(), 1);
    assert!(serde_json::from_value::<Vec<IssueNode>>(serde_json::json!({"message": "x"})).is_err());
}
