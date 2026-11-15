//! The active repositories: an inactive one shows no row, lane, card, or
//! badge count; saved keys outlive repositories that are not listed now;
//! a change naming an unknown or oversized key, or one past the capacity,
//! changes nothing.
mod actions;

use super::*;
use crate::orchestrator::{
    Access, SourceStatus, SyncState,
    repo::{Remote, RepoInfo},
    view::{
        OrchestratorView,
        repo_select::{Choice, Selection, identity},
        rows::Tab,
    },
};
use gpui::{Entity, VisualTestContext};
use std::{collections::HashSet, sync::Arc};

const HOST: &str = "forgejo.example.test";

fn forgejo(repository: &str) -> SourceKey {
    SourceKey {
        provider: Provider::Forgejo,
        host: HOST.into(),
        repository: repository.into(),
    }
}

fn issue(repository: &str, number: u64) -> Item {
    let mut issue = item(
        Provider::Forgejo,
        &number.to_string(),
        "Synthetic issue",
        "2026-10-02T00:00:00Z",
    );
    issue.key.source = forgejo(repository);
    issue.identifier = format!("#{number}");
    issue
}

fn key(repository: &str) -> String {
    identity(&forgejo(repository))
}

/// A checkout of `team/app` and an account listing `team/web` and `team/api`.
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
            checkout.snapshot.repo = Some(RepoInfo {
                repository: crate::orchestrator::Repository::Local("/tmp/app/.git".into()),
                main_root: "/tmp/app".into(),
                beads: false,
                beads_root: None,
                remote: Some(Remote {
                    name: "origin".into(),
                    url: format!("git@{HOST}:Team/App.git"),
                    source: forgejo("Team/App"),
                }),
            });
            checkout.snapshot.sources = vec![SourceStatus {
                key: forgejo("Team/App"),
                state: SyncState::Synced {
                    at: at("2026-10-05T00:00:00Z"),
                    count: 1,
                },
            }];
            checkout.snapshot.access = Some(Access::ReadWrite);
            checkout.snapshot.items = Arc::new(vec![issue("Team/App", 1)]);
            view.add_forgejo_account(HOST.into());
            view.repos.last_mut().unwrap().snapshot.items =
                Arc::new(vec![issue("team/web", 2), issue("team/api", 3)]);
            view.tab = Tab::Issues;
            view.refresh_rows();
            view.prepare_repo_roster(cx);
            view.show_kanban(false, cx);
        })
    });
    cx.run_until_parked();
    (dir, view, cx)
}

/// What each place shows: list rows, board lanes, board cards, and badge items.
fn shown(view: &Entity<OrchestratorView>, cx: &mut VisualTestContext) -> [HashSet<String>; 4] {
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.refresh_rows();
            let _ = view.request_board(cx);
        })
    });
    cx.run_until_parked();
    let (rows, badges) = cx.update(|_, cx| {
        let view = view.read(cx);
        let rows = view.keys().into_iter().collect();
        let badges = view
            .shown_items()
            .map(|item| item.key.canonical())
            .collect();
        (rows, badges)
    });
    let (lanes, cards) = cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let board = view.request_board(cx).unwrap().unwrap();
            let pages = board.pages.as_ref().unwrap();
            let lanes = pages
                .iter()
                .flat_map(|page| &page.lanes)
                .map(|lane| lane.key.clone())
                .collect();
            let cards = pages
                .iter()
                .flat_map(|page| &page.cards)
                .map(|card| card.key.clone())
                .collect();
            (lanes, cards)
        })
    });
    [rows, lanes, cards, badges]
}

#[gpui::test]
fn an_inactive_repository_shows_no_row_lane_card_or_badge(cx: &mut gpui::TestAppContext) {
    let (_dir, view, cx) = view(cx);
    // Rows, cards, and badges name items; lanes name repositories.
    let expected = |repositories: &[(&str, u64)]| {
        let items: HashSet<String> = repositories
            .iter()
            .map(|(repository, number)| issue(repository, *number).key.canonical())
            .collect();
        let lanes = repositories
            .iter()
            .map(|(repository, _)| key(repository))
            .collect();
        [items.clone(), lanes, items.clone(), items]
    };
    let everything = shown(&view, cx);
    assert_eq!(
        everything,
        expected(&[("Team/App", 1), ("team/web", 2), ("team/api", 3)])
    );
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.choose_repos(Choice::Only(vec![key("team/web")]), cx)
                .unwrap();
        })
    });
    assert_eq!(shown(&view, cx), expected(&[("team/web", 2)]));
    cx.update(|_, cx| view.update(cx, |view, cx| view.choose_repos(Choice::All, cx).unwrap()));
    assert_eq!(shown(&view, cx), everything);
}

#[gpui::test]
fn keys_of_repositories_not_listed_now_are_kept(cx: &mut gpui::TestAppContext) {
    let (_dir, view, cx) = view(cx);
    // Saved while another host was connected; that host's repositories,
    // and one whose listing failed, are not listed now.
    let gone = identity(&SourceKey {
        provider: Provider::Github,
        host: "github.example.test".into(),
        repository: "team/elsewhere".into(),
    });
    let hidden_gone = key("team/failed");
    let saved = Selection {
        active: vec![key("team/app"), gone.clone()],
        hidden: vec![key("team/web"), hidden_gone.clone()],
    };
    let reloaded = Selection::from_json(Some(&saved.to_json()));
    assert_eq!(reloaded.as_ref(), Some(&saved));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.repos.last_mut().unwrap().snapshot.error =
                Some(Arc::new(crate::orchestrator::Error::NothingToOpen));
            view.set_repo_selection(reloaded.as_ref(), false, cx);
            view.toggle_repo(&key("team/api"), cx);
            let chosen = view.take_repo_selection().unwrap().unwrap();
            for kept in [&gone, &key("team/app"), &key("team/api")] {
                assert!(chosen.active.contains(kept), "{kept} in {chosen:?}");
            }
            for kept in [&hidden_gone, &key("team/web")] {
                assert!(chosen.hidden.contains(kept), "{kept} in {chosen:?}");
            }
        })
    });
}

#[gpui::test]
fn an_unknown_or_oversized_key_changes_nothing(cx: &mut gpui::TestAppContext) {
    let (_dir, view, cx) = view(cx);
    let oversized = key(&format!("team/{}", "x".repeat(1024)));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.choose_repos(Choice::Only(vec![key("team/web")]), cx)
                .unwrap();
            let before = view.repo_selection().cloned().unwrap();
            for refused in [
                vec![key("team/unknown")],
                vec![key("team/api"), oversized.clone()],
                vec!["forgejo:Forgejo.Example.Test:team/api".into()],
            ] {
                assert!(view.choose_repos(Choice::Only(refused), cx).is_err());
                assert_eq!(view.repo_selection(), Some(&before));
            }
            assert!(
                view.repo_entries()
                    .iter()
                    .all(|entry| entry.active == before.active.contains(&entry.key))
            );
        })
    });
    assert!(!sheprd_control_protocol::protocol::orchestrator::repo_keys_bounded(&[oversized]));
}

#[gpui::test]
fn a_change_past_the_capacity_is_refused_and_keeps_the_saved_keys(cx: &mut gpui::TestAppContext) {
    let (_dir, view, cx) = view(cx);
    // Saved while another host was connected: every key is of a repository
    // not listed now, and the selection is full.
    let saved = Selection {
        active: (0..crate::orchestrator::view::repo_select::MAX_KEYS)
            .map(|index| {
                identity(&SourceKey {
                    provider: Provider::Github,
                    host: "github.example.test".into(),
                    repository: format!("team/gone-{index}"),
                })
            })
            .collect(),
        hidden: Vec::new(),
    };
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.set_repo_selection(Some(&saved), false, cx);
            let before = view.repo_entries().to_vec();
            view.toggle_repo(&key("team/web"), cx);
            assert_eq!(view.repo_selection(), Some(&saved));
            assert!(
                view.choose_repos(Choice::Only(vec![key("team/web")]), cx)
                    .is_err()
            );
            assert_eq!(view.repo_selection(), Some(&saved));
            assert_eq!(view.repo_entries(), before.as_slice());
        })
    });
}
