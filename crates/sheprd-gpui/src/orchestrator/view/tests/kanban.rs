mod history;
mod refresh;

use super::*;
use crate::orchestrator::{SourceStatus, SyncState, view::kanban::rows as board_rows};
use sheprd_control_protocol::protocol::{Response, orchestrator as wire};
use sheprd_orchestrator::kanban::config::Mappings;
use std::sync::Arc;

fn fixture(host: &str, title: &str) -> Item {
    let mut issue = item(Provider::Github, "7", title, "2026-10-04T00:00:00Z");
    issue.key.source.host = host.into();
    issue.key.source.repository = "team/app".into();
    issue
}

fn snapshot(items: Vec<Item>) -> crate::orchestrator::Snapshot {
    crate::orchestrator::Snapshot {
        sources: items
            .iter()
            .map(|item| SourceStatus {
                key: item.key.source.clone(),
                state: SyncState::SignIn,
            })
            .collect(),
        items: Arc::new(items),
        ..Default::default()
    }
}

#[gpui::test]
fn kanban_cross_host_keys_cannot_select_another_source_or_start_an_action(
    cx: &mut gpui::TestAppContext,
) {
    let (_dir, view, cx) = empty::open(cx);
    let one = fixture("github.example.test", "First host");
    let two = fixture("forgejo.example.test", "Second host");
    let (actions, worker) = std::sync::mpsc::sync_channel(1);
    let first_key = one.key.canonical();
    let other_key = two.key.canonical();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.action_sink = Some(actions);
            view.repo_mut().snapshot = snapshot(vec![one]);
            view.refresh_rows();
            view.show_kanban(true, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.open_kanban_key(&first_key, cx);
            assert_eq!(
                view.detail.as_ref().map(|detail| detail.key.as_str()),
                Some(first_key.as_str())
            );
            view.open_kanban_key(&other_key, cx);
            assert_eq!(
                view.detail.as_ref().map(|detail| detail.key.as_str()),
                Some(first_key.as_str())
            );
            assert!(view.kanban_repo_of(&other_key).is_none());
            view.act(crate::orchestrator::Action::Remove {
                run: "unapproved".into(),
            });
            assert!(matches!(
                worker.try_recv(),
                Err(std::sync::mpsc::TryRecvError::Empty)
            ));
            // A changed source cannot inherit an earlier prepared route.
            view.repo_mut().snapshot = snapshot(vec![two]);
            view.open_kanban_key(&first_key, cx);
            assert!(view.item(&first_key).is_none());
            // Prove the sink would observe a dispatched action outside the board.
            view.show_kanban(false, cx);
            view.act(crate::orchestrator::Action::Remove {
                run: "control".into(),
            });
            assert!(matches!(
                worker.try_recv(),
                Ok(crate::orchestrator::Action::Remove { .. })
            ));
        })
    });
}

#[test]
fn kanban_pages_bound_encoded_data_and_cut_an_oversized_title() {
    let prepare = |items| {
        board_rows::prepare(
            vec![board_rows::Input {
                worker: "test".into(),
                snapshot: snapshot(items),
                account: None,
            }],
            Mappings::default(),
            board_rows::Filter {
                query: String::new(),
                projects: Default::default(),
                backends: Default::default(),
            },
            1,
            None,
            None,
            board_rows::LaneOrder::default(),
        )
    };
    let mut items = Vec::new();
    for id in 0..wire::MAX_PAGE_CARDS * 2 {
        let mut item = fixture("github.example.test", &"x".repeat(4000));
        item.key.native_id = id.to_string();
        item.key.source.repository = format!("team/app{id}");
        items.push(item);
    }
    let prepared = prepare(items).unwrap();
    for page in prepared.pages.unwrap() {
        assert!(page.cards.len() <= wire::MAX_PAGE_CARDS);
        assert!(
            serde_json::to_vec(&Response::Orchestrator {
                reply: wire::BoardReply::Page(Box::new(page))
            })
            .unwrap()
            .len()
                <= wire::MAX_PAGE_BYTES
        );
    }
    // Escaped controls and multi-byte characters exercise encoded, not raw, bounds.
    let oversized = fixture(
        "github.example.test",
        &"\u{0000}\u{00e9}".repeat(wire::MAX_PAGE_BYTES),
    );
    let full_title = oversized.title.clone();
    let key = oversized.key.canonical();
    let normal = fixture("forgejo.example.test", "Still available");
    let normal_key = normal.key.canonical();
    let prepared = prepare(vec![oversized, normal]).unwrap();
    let pages = prepared.pages.unwrap();
    let cards: Vec<_> = pages.iter().flat_map(|page| &page.cards).collect();
    let cut = cards.iter().find(|card| card.key == key).unwrap();
    assert!(full_title.starts_with(cut.title.strip_suffix("...").unwrap()));
    assert!(cut.title.len() < full_title.len());
    assert!(
        cut.warnings
            .iter()
            .any(|warning| warning == "title_truncated")
    );
    assert!(cards.iter().any(|card| card.key == normal_key));
    for page in pages {
        assert!(page.cards.len() <= wire::MAX_PAGE_CARDS);
        assert!(
            serde_json::to_vec(&Response::Orchestrator {
                reply: wire::BoardReply::Page(Box::new(page))
            })
            .unwrap()
            .len()
                <= wire::MAX_PAGE_BYTES
        );
    }
}
