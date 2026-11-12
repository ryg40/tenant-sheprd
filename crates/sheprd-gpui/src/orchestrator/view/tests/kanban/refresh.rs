use super::*;

#[gpui::test]
fn kanban_refresh_keeps_the_selected_popup_until_complete_removal(cx: &mut gpui::TestAppContext) {
    let (_dir, view, cx) = empty::open(cx);
    let mut issue = fixture("forgejo.example.test", "Selected issue");
    issue.key.source.provider = Provider::Forgejo;
    let key = issue.key.canonical();
    let source = issue.key.source.clone();
    let complete = |items| crate::orchestrator::Snapshot {
        sources: vec![SourceStatus {
            key: source.clone(),
            state: SyncState::Synced {
                at: Utc::now(),
                count: 0,
            },
        }],
        items: Arc::new(items),
        ..Default::default()
    };
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.repo_mut().snapshot = complete(vec![issue.clone()]);
            view.refresh_rows();
            view.show_kanban(true, cx);
            window.focus(&view.focus, cx);
        })
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            assert_eq!(view.detail.as_ref().map(|detail| &detail.key), Some(&key));
            assert_eq!(view.kanban_detail_item(&key), Some(&issue));
            let mut changed = issue.clone();
            changed.description = Some("Updated while open".into());
            view.repo_mut().snapshot = complete(vec![changed]);
            view.refresh_rows();
            view.prepare_kanban(cx);
            assert!(view.kanban_detail_item(&key).is_some());
            assert_eq!(view.detail.as_ref().map(|detail| &detail.key), Some(&key));
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            assert_eq!(view.detail.as_ref().map(|detail| &detail.key), Some(&key));
            assert_eq!(
                view.kanban_detail_item(&key)
                    .and_then(|item| item.description.as_deref()),
                Some("Updated while open")
            );
            let mut other = issue.clone();
            other.key.native_id = "8".into();
            let mut partial = complete(vec![other]);
            partial.sources[0].state = SyncState::Capped { count: 1 };
            view.repo_mut().snapshot = partial;
            view.refresh_rows();
            view.prepare_kanban(cx);
        })
    });
    cx.run_until_parked();
    // A history failure publishes another board while the active list is incomplete.
    for failed in [true, false] {
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                assert_eq!(view.detail.as_ref().map(|detail| &detail.key), Some(&key));
                assert!(view.kanban_detail_item(&key).is_some());
                assert!(view.kanban.keys().contains(&key));
                view.repo_mut().snapshot.history_error = failed.then(|| {
                    Arc::new(sheprd_orchestrator::Error::Kanban(
                        sheprd_orchestrator::kanban::Error::HistoryIncomplete,
                    ))
                });
                view.refresh_rows();
                view.prepare_kanban(cx);
            })
        });
        cx.run_until_parked();
    }
    // Closing does not clear selection: Enter must reopen that same retained card.
    cx.simulate_keystrokes("escape enter");
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            assert_eq!(view.detail.as_ref().map(|detail| &detail.key), Some(&key));
            assert!(view.kanban.keys().contains(&key));
            view.repo_mut().snapshot = complete(Vec::new());
            view.refresh_rows();
            view.prepare_kanban(cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            assert!(view.detail.is_none());
            assert!(!view.kanban.keys().contains(&key));
        })
    });

    // A suppressed account copy cannot outlive a complete checkout removal,
    // even when the account's source uses a different repository spelling.
    for repository in ["team/app", "Team/App"] {
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.repos[0].snapshot = complete(vec![issue.clone()]);
                view.add_forgejo_account(source.host.clone());
                let account = view.repos.last_mut().unwrap();
                let mut copy = issue.clone();
                copy.key.source.repository = repository.into();
                copy.description = Some("Older account copy".into());
                account.snapshot.items = Arc::new(vec![copy]);
                account.snapshot.sources[0].state = SyncState::Synced {
                    at: Utc::now(),
                    count: 1,
                };
                view.refresh_rows();
                view.prepare_kanban(cx);
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.open_kanban_key(&key, cx);
                assert_eq!(view.kanban_detail_item(&key), Some(&issue));
                let mut partial = complete(Vec::new());
                partial.sources[0].state = SyncState::Syncing { count: None };
                view.repos[0].snapshot = partial;
                view.refresh_rows();
                view.prepare_kanban(cx);
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                assert_eq!(view.detail.as_ref().map(|detail| &detail.key), Some(&key));
                assert_eq!(view.kanban_detail_item(&key), Some(&issue));
                view.repos[0].snapshot = complete(Vec::new());
                view.refresh_rows();
                view.prepare_kanban(cx);
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| view.update(cx, |view, _| assert!(view.detail.is_none())));
    }

    // A still-listed source without metadata is incomplete, not removed.
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.repos.retain(|repo| repo.remote.is_none());
            view.repo_mut().snapshot = complete(vec![issue.clone()]);
            view.refresh_rows();
            view.prepare_kanban(cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| view.update(cx, |view, cx| view.open_kanban_key(&key, cx)));
    for _ in 0..2 {
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.repo_mut().snapshot = Default::default();
                view.refresh_rows();
                view.prepare_kanban(cx);
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |view, _| {
                assert_eq!(view.detail.as_ref().map(|detail| &detail.key), Some(&key));
                assert_eq!(view.kanban_detail_item(&key), Some(&issue));
            })
        });
    }
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.set_repos(Vec::new(), cx);
            view.prepare_kanban(cx);
            assert_eq!(view.detail.as_ref().map(|detail| &detail.key), Some(&key));
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| view.update(cx, |view, _| assert!(view.detail.is_none())));
}
