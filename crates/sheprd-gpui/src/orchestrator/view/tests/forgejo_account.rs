//! A Forgejo account lists the open issues of repositories no checkout
//! holds, each named and picked by its own `owner/name`, and leaves a
//! checkout's repository to that checkout. Its rows have no checkout, so
//! they offer no action but their web page.
use super::*;
use crate::orchestrator::{
    Access, RemoteScope, SourceStatus, SyncState,
    repo::{Remote, RepoInfo},
    view::{OrchestratorView, projects::Entry, rows::Tab},
};
use gpui::{Entity, VisualTestContext};
use std::sync::Arc;

const HOST: &str = "forgejo.example.com";

fn forgejo(repository: &str) -> SourceKey {
    SourceKey {
        provider: Provider::Forgejo,
        host: HOST.into(),
        repository: repository.into(),
    }
}

fn issue(repository: &str, number: u64, updated: &str) -> Item {
    let mut issue = item(Provider::Forgejo, &number.to_string(), "Fix it", updated);
    issue.key.source = forgejo(repository);
    issue.identifier = format!("#{number}");
    issue.url = Some(format!("https://{HOST}/{repository}/issues/{number}"));
    issue
}

/// A checkout of `team/app`, whose remote is `Team/App` as it was cloned,
/// listing its issue 1, and the account listing that issue again with
/// `team/web`'s 1 and `team/api`'s 2.
fn view(
    cx: &mut gpui::TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<OrchestratorView>,
    &mut VisualTestContext,
) {
    let (dir, view, cx) = empty::open(cx);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let checkout = view.repo_mut();
            let source = forgejo("Team/App");
            checkout.snapshot.repo = Some(RepoInfo {
                repository: crate::orchestrator::Repository::Local("/tmp/app/.git".into()),
                main_root: "/tmp/app".into(),
                beads: false,
                beads_root: None,
                remote: Some(Remote {
                    name: "origin".into(),
                    url: format!("git@{HOST}:Team/App.git"),
                    source: source.clone(),
                }),
            });
            checkout.snapshot.sources = vec![SourceStatus {
                key: source,
                state: SyncState::Synced {
                    at: at("2026-10-05T00:00:00Z"),
                    count: 1,
                },
            }];
            checkout.snapshot.access = Some(Access::ReadWrite);
            checkout.snapshot.items = Arc::new(vec![issue("Team/App", 1, "2026-10-01T00:00:00Z")]);
            view.add_forgejo_account(HOST.into());
            // Asked again, as a second probe would, it is still one entry.
            view.add_forgejo_account(HOST.into());
            let account = view.repos.last_mut().unwrap();
            assert!(account.lists_account());
            // Newer than the checkout's copy, which still keeps the row.
            account.snapshot.items = Arc::new(vec![
                issue("team/app", 1, "2026-10-04T00:00:00Z"),
                issue("team/web", 1, "2026-10-03T00:00:00Z"),
                issue("team/api", 2, "2026-10-02T00:00:00Z"),
            ]);
            view.tab = Tab::Issues;
            view.refresh_rows();
            cx.notify();
        })
    });
    cx.run_until_parked();
    (dir, view, cx)
}

#[gpui::test]
fn the_account_lists_repositories_without_a_checkout_once(cx: &mut gpui::TestAppContext) {
    let (_dir, view, cx) = view(cx);
    cx.update(|_, cx| {
        let view = view.read(cx);
        let account_repos = view.repos.iter().filter(|repo| repo.lists_account());
        assert_eq!(account_repos.count(), 1);
        let keys = view.keys();
        let app = issue("Team/App", 1, "2026-10-01T00:00:00Z").key.canonical();
        let web = issue("team/web", 1, "2026-10-03T00:00:00Z").key.canonical();
        let api = issue("team/api", 2, "2026-10-02T00:00:00Z").key.canonical();
        assert_eq!(keys.len(), 3, "{keys:?}");
        for key in [&app, &web, &api] {
            assert!(keys.contains(key), "{key} in {keys:?}");
        }
        // The checkout's own issue is the checkout's row, with its actions.
        assert_eq!(view.repo_of(&app), Some(0));
        assert!(!view.repos[0].lists_account());
        let account = view.repo_of(&web).unwrap();
        assert!(view.repos[account].lists_account());
        assert!(view.listing_projects());
        let mut projects: Vec<String> = view
            .lines
            .iter()
            .filter_map(|line| match line {
                Line::Row(row) if view.repos[row.repo].lists_account() => {
                    Some(view.row_project(row))
                }
                _ => None,
            })
            .collect();
        projects.sort();
        assert_eq!(projects, ["team/api", "team/web"]);
    });
}

#[gpui::test]
fn each_account_repository_is_a_project_to_pick(cx: &mut gpui::TestAppContext) {
    let (_dir, view, cx) = view(cx);
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            let labels: Vec<String> = view
                .project_entries()
                .into_iter()
                .map(|entry| match entry {
                    Entry::Repo { label, .. } => label,
                    other => panic!("{other:?}"),
                })
                .collect();
            // The checkout, then the account's other repositories by name:
            // never the account itself, nor the checkout's repository again.
            assert_eq!(labels, ["App", "team/api", "team/web"]);
            let Some(Entry::Repo { key, .. }) = view.project_entries().into_iter().nth(2) else {
                panic!("no team/web entry");
            };
            view.projects.insert(key);
            view.refresh_rows();
            assert_eq!(
                view.keys(),
                [issue("team/web", 1, "2026-10-03T00:00:00Z").key.canonical()]
            );
            assert_eq!(view.project_filter_label(), "team/web");
        })
    });
}

#[gpui::test]
fn an_account_row_offers_no_action_but_its_web_page(cx: &mut gpui::TestAppContext) {
    let (_dir, view, cx) = view(cx);
    let web = issue("team/web", 1, "2026-10-03T00:00:00Z").key.canonical();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            // Only the account's rows are listed.
            view.repos[0].snapshot.items = Arc::new(Vec::new());
            view.refresh_rows();
            cx.notify();
        })
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("orchestrator-start").is_none());
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.open_detail(web.clone(), cx);
            assert!(view.repos[view.active].lists_account());
            assert_eq!(view.snapshot().access, None);
        })
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("orchestrator-detail-web").is_some());
    assert!(cx.debug_bounds("orchestrator-start").is_none());
}

#[gpui::test]
fn new_checkouts_keep_the_account_and_its_picks(cx: &mut gpui::TestAppContext) {
    let (_dir, view, cx) = view(cx);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let pick =
                crate::orchestrator::view::projects::forgejo_repository(&forgejo("team/web"));
            view.projects.insert(pick.clone());
            view.set_repos(Vec::new(), cx);
            assert_eq!(view.picked_projects(), [pick]);
            assert_eq!(view.repos.len(), 1, "the account outlives the checkouts");
            assert!(matches!(
                view.repos[0].remote,
                Some(RemoteScope::ForgejoAccount { ref host }) if host == HOST
            ));
        })
    });
}
