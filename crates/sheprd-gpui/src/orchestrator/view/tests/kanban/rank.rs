use super::*;
use crate::orchestrator::view::kanban::{error::Error, order, rank};
use sheprd_orchestrator::kanban::history::{Collection, Origin, Read, Sample, Status, WINDOW};
use std::collections::{HashMap, HashSet};

fn lane(name: &str) -> sheprd_orchestrator::kanban::identity::LaneKey {
    order::parse(&format!("forgejo:forgejo.example.test:team/{name}")).unwrap()
}

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|id| (*id).to_owned()).collect()
}

#[test]
fn kanban_rank_moves_admits_new_items_prunes_only_lanes_and_bounds() {
    let app = lane("app");
    let empty = rank::Ranks::default();
    // A move ranks the cards it passes: c to the top of [a, b | c] keeps a and b ranked.
    let ranks = empty.set(&app, ids(&["a", "b"])).unwrap();
    let cell = ids(&["a", "b", "c", "d"]);
    let moved = ranks.arrange(&app, &cell, "c", 0).unwrap();
    assert_eq!(moved.get(&app), ids(&["c", "a", "b"]).as_slice());
    // Down past an unranked card ranks it too; later cards stay unranked.
    let down = ranks.arrange(&app, &cell, "a", 2).unwrap();
    assert_eq!(down.get(&app), ids(&["b", "c", "a"]).as_slice());
    // Ranks of another cell keep their place between this cell's slots.
    let mixed = empty.set(&app, ids(&["x", "a", "y", "b"])).unwrap();
    let swapped = mixed.arrange(&app, &cell, "b", 0).unwrap();
    assert_eq!(swapped.get(&app), ids(&["x", "b", "y", "a"]).as_slice());
    assert!(matches!(
        ranks.arrange(&app, &cell, "unknown", 0),
        Err(Error::UnknownCard)
    ));

    // A new item is unranked and numbers count only open, ranked items.
    let open = HashMap::from([(
        app.clone(),
        ids(&["c", "b", "new"]).into_iter().collect::<HashSet<_>>(),
    )]);
    let numbers = rank::numbers(&moved, &open);
    assert_eq!(numbers.get(&(app.clone(), "c".into())), Some(&1));
    assert_eq!(numbers.get(&(app.clone(), "a".into())), None);
    assert_eq!(numbers.get(&(app.clone(), "b".into())), Some(&2));
    assert_eq!(numbers.get(&(app.clone(), "new".into())), None);

    // A lane's ranks leave with the lane, on an authoritative roster only.
    assert_eq!(moved.reconcile(std::slice::from_ref(&app), true), moved);
    assert_eq!(moved.reconcile(&[], false), moved);
    assert!(moved.reconcile(&[], true).get(&app).is_empty());

    // Bounds: items per lane, total items, bytes per ID, and lanes.
    let many: Vec<String> = (0..=rank::MAX_ITEMS).map(|at| at.to_string()).collect();
    assert!(matches!(
        empty.set(&app, many.clone()),
        Err(Error::RankLimit)
    ));
    assert_eq!(
        rank::Ranks::bounded([(app.clone(), many)]).get(&app).len(),
        rank::MAX_ITEMS
    );
    let long = "7".repeat(order::MAX_KEY_BYTES + 1);
    assert!(matches!(
        empty.set(&app, vec![long.clone()]),
        Err(Error::OrderLimit)
    ));
    assert!(
        rank::Ranks::bounded([(app.clone(), vec![long])])
            .get(&app)
            .is_empty()
    );
    let full: Vec<_> = (0..=order::MAX_LANES)
        .map(|at| (lane(&format!("l{at}")), ids(&["1"])))
        .collect();
    let bounded = rank::Ranks::bounded(full.clone());
    assert_eq!(bounded.lanes().count(), order::MAX_LANES);
    let last = &full[order::MAX_LANES].0;
    assert!(matches!(
        bounded.set(last, ids(&["1"])),
        Err(Error::RankLimit)
    ));
    let wide: Vec<_> = (0..rank::MAX_TOTAL / rank::MAX_ITEMS + 1)
        .map(|at| {
            let items = (0..rank::MAX_ITEMS).map(|id| id.to_string()).collect();
            (lane(&format!("w{at}")), items)
        })
        .collect();
    let total: usize = rank::Ranks::bounded(wide)
        .lanes()
        .map(|(_, ids)| ids.len())
        .sum();
    assert_eq!(total, rank::MAX_TOTAL);
}

#[gpui::test]
fn kanban_rank_survives_refresh_and_refusals_change_nothing(cx: &mut gpui::TestAppContext) {
    let (_dir, view, cx) = empty::open(cx);
    let issue = |id: &str, title: &str| {
        let mut issue = fixture("github.example.test", title);
        issue.key.native_id = id.into();
        issue.identifier = format!("#{id}");
        issue
    };
    let items = vec![
        issue("1", "First"),
        issue("2", "Second"),
        issue("3", "Third"),
    ];
    let key = |at: usize| items[at].key.canonical();
    let app = order::parse(&items[0].key.source.canonical()).unwrap();
    let ranks =
        |view: &mut crate::orchestrator::view::OrchestratorView,
         cx: &mut gpui::Context<crate::orchestrator::view::OrchestratorView>| {
            let ready = view.request_board(cx).ok().flatten().unwrap();
            let pages = ready.pages.as_ref().unwrap();
            pages
                .iter()
                .flat_map(|page| &page.cards)
                .map(|card| (card.key.clone(), card.rank))
                .collect::<HashMap<_, _>>()
        };
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.repo_mut().snapshot = snapshot(items.clone());
            view.refresh_rows();
            view.show_kanban(true, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.rank_lane(&app, &[key(2), key(0)], cx).unwrap();
        })
    });
    cx.run_until_parked();
    let offset = gpui::point(gpui::px(0.), gpui::px(-40.));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.take_lane_order();
            view.open_kanban_key(&key(0), cx);
        })
    });
    // Opening reveals the card first; the refresh must then keep the scroll.
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.kanban.scroll.0.borrow().base_handle.set_offset(offset);
            // New snapshot data: a changed title and a new item.
            let mut next = items.clone();
            next[1].title = "Second, renamed".into();
            next.push(issue("4", "Fourth"));
            view.repo_mut().snapshot = snapshot(next);
            view.refresh_rows();
            view.prepare_kanban(cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let shown = ranks(view, cx);
            assert_eq!(shown[&key(2)], Some(1));
            assert_eq!(shown[&key(0)], Some(2));
            assert_eq!(shown[&key(1)], None);
            let mut fourth = items[0].clone();
            fourth.key.native_id = "4".into();
            assert_eq!(shown.get(&fourth.key.canonical()), Some(&None));
            assert_eq!(
                view.detail.as_ref().map(|detail| &detail.key),
                Some(&key(0))
            );
            assert_eq!(view.kanban.scroll.0.borrow().base_handle.offset(), offset);
            let saved = view.kanban.ranks.clone();

            // The command refuses an unknown lane or item and changes nothing.
            let other = order::parse("github:github.example.test:team/other").unwrap();
            assert!(matches!(
                view.rank_lane(&other, &[key(0)], cx),
                Err(Error::UnknownLane)
            ));
            let mut stranger = items[0].clone();
            stranger.key.native_id = "99".into();
            for wrong in [stranger.key.canonical(), "not a key".into()] {
                assert!(matches!(
                    view.rank_lane(&app, &[key(1), wrong], cx),
                    Err(Error::UnknownCard)
                ));
            }
            assert_eq!(view.kanban.ranks, saved);
            assert_eq!(ranks(view, cx), shown);

            // A hidden lane keeps its saved ranks, but neither the command,
            // a rank key, nor a drop on one of its cards changes them.
            let selection = crate::orchestrator::view::repo_select::Selection {
                active: Vec::new(),
                hidden: vec![app.canonical()],
            };
            view.set_repo_selection(Some(&selection), false, cx);
        })
    });
    cx.run_until_parked();
    let saved = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let saved = view.kanban.ranks.clone();
            assert!(!saved.index().is_empty());
            assert!(matches!(
                view.rank_lane(&app, &[key(1)], cx),
                Err(Error::UnknownLane)
            ));
            assert!(matches!(
                view.rank_lane(&app, &[], cx),
                Err(Error::UnknownLane)
            ));
            assert!(view.rank_card(&key(0), 1, cx).is_err());
            // Hiding closed the selected card, so the rank keys move nothing.
            for key in ["alt-shift-down", "alt-shift-up", "alt-shift-backspace"] {
                view.kanban_key(
                    &gpui::KeyDownEvent {
                        keystroke: gpui::Keystroke::parse(key).unwrap(),
                        is_held: false,
                        prefer_character_input: false,
                    },
                    window,
                    cx,
                );
            }
            assert_eq!(view.kanban.ranks, saved);
            view.set_repo_selection(None, false, cx);
            saved
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            assert_eq!(view.kanban.ranks, saved);
            let shown = ranks(view, cx);
            assert_eq!(shown[&key(2)], Some(1));
            assert_eq!(shown[&key(0)], Some(2));
        })
    });
}

/// A Forgejo issue of `team/app`, which has a history reader.
fn tracked(id: &str) -> Item {
    let mut issue = fixture("forgejo.example.test", id);
    issue.key.source.provider = Provider::Forgejo;
    issue.key.native_id = id.into();
    issue.identifier = format!("#{id}");
    issue
}

/// A checkout whose active listing is complete, with `history` beside it.
fn authoritative(items: Vec<Item>, history: Vec<Status>) -> crate::orchestrator::Snapshot {
    let mut snapshot = snapshot(items);
    for status in &mut snapshot.sources {
        status.state = SyncState::Synced {
            at: Utc::now() - chrono::TimeDelta::minutes(1),
            count: 0,
        };
    }
    snapshot.repo = Some(crate::orchestrator::repo::RepoInfo {
        repository: crate::orchestrator::Repository::Local("/r/.git".into()),
        main_root: "/r".into(),
        beads: false,
        beads_root: None,
        remote: None,
    });
    snapshot.history = Arc::new(history);
    snapshot
}

#[gpui::test]
fn kanban_rank_survives_absence_from_every_read(cx: &mut gpui::TestAppContext) {
    let (_dir, view, cx) = empty::open(cx);
    let (first, second) = (tracked("1"), tracked("2"));
    let app = order::parse(&first.key.source.canonical()).unwrap();
    let collection = Collection::Repository(first.key.source.clone());
    let saved = ids(&["2", "1"]);
    let show = |items: Vec<Item>, history: Vec<Status>, cx: &mut gpui::VisualTestContext| {
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.repo_mut().snapshot = authoritative(items, history);
                view.refresh_rows();
                view.prepare_kanban(cx);
            })
        });
        cx.run_until_parked();
    };
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.kanban.account_ready = true;
            let ranks = rank::Ranks::default().set(&app, saved.clone()).unwrap();
            let keys = order::Keys {
                lanes: vec![app.clone()],
                ranks,
            };
            view.set_lane_order(&keys, true, cx);
            view.repo_mut().snapshot = authoritative(vec![first.clone(), second.clone()], vec![]);
            view.refresh_rows();
            view.show_kanban(true, cx);
        })
    });
    cx.run_until_parked();
    let now = Utc::now();
    let read = |truncated| Status {
        collection: collection.clone(),
        failure: None,
        origin: Some(Origin::Live),
        sample: Some(
            Sample::from_read(
                collection.clone(),
                Read {
                    complete: true,
                    cutoff: now - WINDOW,
                    records: Vec::new(),
                    truncated,
                },
                now,
            )
            .unwrap(),
        ),
        supported: true,
    };
    let failed = Status {
        collection: collection.clone(),
        failure: Some(Arc::new(sheprd_orchestrator::Error::Kanban(
            sheprd_orchestrator::kanban::Error::HistoryIncomplete,
        ))),
        origin: None,
        sample: None,
        supported: true,
    };
    // Item 2 leaves the complete active listing. History fails, is capped,
    // or is complete, live, and untruncated without it: item 2 may have
    // closed and reopened between the reads. No read proves it left.
    for history in [failed, read(true), read(false)] {
        show(vec![first.clone(), second.clone()], vec![], cx);
        show(vec![first.clone()], vec![history.clone()], cx);
        show(vec![first.clone()], vec![history], cx);
        cx.update(|_, cx| {
            view.update(cx, |view, _| {
                assert_eq!(view.kanban.ranks.get(&app), saved.as_slice());
            })
        });
    }
}

#[gpui::test]
fn kanban_rank_keys_refuse_during_a_card_drag_and_escape_discards_it(
    cx: &mut gpui::TestAppContext,
) {
    let (_dir, view, cx) = empty::open(cx);
    let items = vec![tracked("1"), tracked("2"), tracked("3")];
    let app = order::parse(&items[0].key.source.canonical()).unwrap();
    let keys: Vec<_> = items.iter().map(|item| item.key.canonical()).collect();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.repo_mut().snapshot = snapshot(items.clone());
            view.refresh_rows();
            view.show_kanban(true, cx);
            window.focus(&view.focus, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.rank_lane(&app, &[keys[1].clone(), keys[0].clone()], cx)
                .unwrap();
            // A clean baseline: the final check sees only what the drag saves.
            view.take_lane_order();
        })
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let before = view.kanban.ranks.clone();
            let card =
                sheprd_orchestrator::kanban::identity::CardKey::try_from(&items[1].key).unwrap();
            view.kanban.drag.active_card =
                Some(crate::orchestrator::view::kanban::drag::CardDrag {
                    board: cx.entity_id(),
                    card,
                    revision: view.kanban.revision,
                });
            for key in [
                "alt-shift-down",
                "alt-shift-backspace",
                "alt-shift-up",
                "escape",
            ] {
                view.kanban_key(
                    &gpui::KeyDownEvent {
                        keystroke: gpui::Keystroke::parse(key).unwrap(),
                        is_held: false,
                        prefer_character_input: false,
                    },
                    window,
                    cx,
                );
            }
            assert!(view.kanban.drag.active_card.is_none());
            assert_eq!(view.kanban.ranks, before);
            assert!(view.take_lane_order().is_none());
        })
    });
}
