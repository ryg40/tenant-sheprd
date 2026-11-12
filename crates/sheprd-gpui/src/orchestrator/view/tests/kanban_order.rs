use super::*;
use crate::orchestrator::{
    SourceStatus, SyncState,
    view::kanban::{drag::LaneDrag, error::Error, order},
};
use gpui::AppContext;
use std::sync::Arc;

#[gpui::test]
fn kanban_unknown_cross_window_and_cancelled_drags_preserve_order_and_tracker(
    cx: &mut gpui::TestAppContext,
) {
    let (_dir, view, cx) = empty::open(cx);
    let mut first = item(
        Provider::Github,
        "7",
        "Synthetic ticket",
        "2026-10-04T00:00:00Z",
    );
    first.key.source.host = "github.example.test".into();
    first.key.source.repository = "team/one".into();
    let mut second = first.clone();
    second.key.source.repository = "team/two".into();
    let one = order::parse(&first.key.source.canonical()).unwrap();
    let two = order::parse(&second.key.source.canonical()).unwrap();
    let before = vec![first, second];
    let (actions, worker) = std::sync::mpsc::sync_channel(1);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.action_sink = Some(actions);
            view.repo_mut().snapshot = crate::orchestrator::Snapshot {
                sources: before
                    .iter()
                    .map(|item| SourceStatus {
                        key: item.key.source.clone(),
                        state: SyncState::SignIn,
                    })
                    .collect(),
                items: Arc::new(before.clone()),
                ..Default::default()
            };
            view.refresh_rows();
            view.show_kanban(true, cx);
        })
    });
    cx.run_until_parked();
    let foreign = cx.new(|_| ());
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let original = view.kanban.order.clone();
            view.take_lane_order();
            let unknown = order::parse("github:github.example.test:team/unknown").unwrap();
            assert!(matches!(
                view.order_lane(&unknown, None, cx),
                Err(Error::UnknownLane)
            ));
            assert!(matches!(
                view.order_lane(&one, Some(&unknown), cx),
                Err(Error::UnknownLane)
            ));
            let drag = LaneDrag {
                board: foreign.entity_id(),
                lane: two.clone(),
                revision: view.kanban.revision,
            };
            view.kanban.drag.active = Some(drag.clone());
            assert!(matches!(
                view.drop_lane(&drag, Some(&one), cx),
                Err(Error::UnknownLane)
            ));
            let drag = LaneDrag {
                board: cx.entity_id(),
                ..drag
            };
            view.kanban.drag.active = Some(drag.clone());
            view.kanban_key(
                &gpui::KeyDownEvent {
                    keystroke: gpui::Keystroke::parse("escape").unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                },
                window,
                cx,
            );
            assert!(matches!(
                view.drop_lane(&drag, Some(&one), cx),
                Err(Error::UnknownLane)
            ));
            assert_eq!(view.kanban.order, original);
            assert!(view.take_lane_order().is_none());
            assert_eq!(*view.repo_mut().snapshot.items, before);
            assert!(matches!(
                worker.try_recv(),
                Err(std::sync::mpsc::TryRecvError::Empty)
            ));
            // The valid path keeps the open card and changes local order only.
            let key = before[0].key.canonical();
            view.open_kanban_key(&key, cx);
            view.order_lane(&two, Some(&one), cx).unwrap();
            assert_eq!(view.detail.as_ref().map(|detail| &detail.key), Some(&key));
            assert_eq!(view.kanban_detail_item(&key), Some(&before[0]));
            // Retained rows may still paint, but cannot authorize another move.
            assert!(matches!(
                view.order_lane(&one, None, cx),
                Err(Error::Pending)
            ));
            assert_eq!(view.kanban.order, vec![two, one]);
            assert_eq!(*view.repo_mut().snapshot.items, before);
            assert!(matches!(
                worker.try_recv(),
                Err(std::sync::mpsc::TryRecvError::Empty)
            ));
        })
    });
}
