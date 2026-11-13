use super::*;
use crate::orchestrator::view::{OrchestratorView, kanban::drag::CardDrag};
use gpui::AppContext;
use sheprd_orchestrator::{
    SourceStatus, SyncState,
    kanban::{
        facts::{LabelCompleteness, NativeStateProvenance, WorkflowFacts},
        moves::{Attempt, Context, Fresh, Identity, Label, LabelId, Labels, MovePlan, Mutation},
    },
    service::MovePhase,
};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub(in crate::orchestrator::view) struct Peer {
    pub fresh: Arc<Mutex<Option<Fresh>>>,
    pub outcome: Arc<Mutex<Option<Failure>>>,
    pub writes: Arc<Mutex<Vec<Mutation>>>,
}

#[derive(Clone, Copy)]
pub(in crate::orchestrator::view) enum Failure {
    Refused,
    Partial,
    Unknown,
}

impl Peer {
    pub fn read(&self) -> sheprd_orchestrator::Result<Fresh> {
        if matches!(*self.outcome.lock().unwrap(), Some(Failure::Unknown))
            && !self.writes.lock().unwrap().is_empty()
        {
            return Err(sheprd_orchestrator::kanban::Error::StaleMove.into());
        }
        Ok(self.fresh.lock().unwrap().as_ref().unwrap().clone())
    }
    pub fn prepare(&self, mut context: Context, item: Item) -> Fresh {
        context.account = "1:test-owner".into();
        let fresh = Fresh {
            context,
            facts: WorkflowFacts {
                labels: Some(LabelCompleteness::Complete),
                native_state: Some(NativeStateProvenance::Tracker),
                settled_at: None,
            },
            identity: Identity::Forgejo {
                repository: 1,
                issue: 7,
            },
            item,
            labels: Labels {
                in_progress: Some(Label {
                    id: LabelId::Forgejo(10),
                    name: "status: in-progress".into(),
                    exclusive: false,
                }),
                in_review: Some(Label {
                    id: LabelId::Forgejo(20),
                    name: "status: in-review".into(),
                    exclusive: false,
                }),
            },
        };
        *self.fresh.lock().unwrap() = Some(fresh.clone());
        fresh
    }
    pub fn mutate(&self, plan: &MovePlan, mutation: &Mutation) -> Attempt {
        self.writes.lock().unwrap().push(mutation.clone());
        match *self.outcome.lock().unwrap() {
            Some(Failure::Refused) => {
                return Attempt::Refused(Arc::new(
                    sheprd_orchestrator::kanban::Error::ReadOnlyMove.into(),
                ));
            }
            Some(Failure::Unknown) => {
                return Attempt::Unknown(Arc::new(
                    sheprd_orchestrator::kanban::Error::StaleMove.into(),
                ));
            }
            Some(Failure::Partial) if matches!(mutation, Mutation::SetState(_)) => {
                return Attempt::Refused(Arc::new(
                    sheprd_orchestrator::kanban::Error::ReadOnlyMove.into(),
                ));
            }
            _ => {}
        }
        let mut held = self.fresh.lock().unwrap();
        let item = &mut held.as_mut().unwrap().item;
        match mutation {
            Mutation::AddLabel(id) => item.labels.push(plan.label_name(id).unwrap().into()),
            Mutation::RemoveLabel(id) => item
                .labels
                .retain(|name| name != plan.label_name(id).unwrap()),
            Mutation::SetState(state) => item.state = state.as_str().into(),
        }
        Attempt::Applied
    }
}

fn move_item(repository: &str) -> Item {
    let mut issue = item(
        Provider::Forgejo,
        "7",
        "Synthetic move",
        "2026-11-12T00:00:00Z",
    );
    issue.key.source.host = "forgejo.example.test".into();
    issue.key.source.repository = repository.into();
    issue.labels.clear();
    issue.state = "open".into();
    issue
}

fn wait(view: &gpui::Entity<OrchestratorView>, cx: &mut gpui::VisualTestContext, writing: bool) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let ready = view.read_with(cx, |view, _| {
            view.repos
                .iter()
                .filter_map(|repo| repo.service.as_ref())
                .flat_map(|service| service.moves().snapshot())
                .any(|s| {
                    if writing {
                        matches!(s.phase, MovePhase::Finished(_))
                    } else {
                        matches!(s.phase, MovePhase::AwaitingConfirmation(_))
                    }
                })
        });
        if ready {
            break;
        }
        assert!(std::time::Instant::now() < end);
        std::thread::yield_now();
    }
    view.update(cx, |view, cx| view.poll_move_dialog(cx));
    cx.run_until_parked();
}

#[gpui::test]
fn kanban_proposals_and_dismissals_never_write_and_native_confirmation_writes_once(
    cx: &mut gpui::TestAppContext,
) {
    let (_dir, view, cx) = empty::open(cx);
    let issue = move_item("test/move-dialog-proposals");
    let key = issue.key.canonical();
    let lane = sheprd_orchestrator::kanban::identity::LaneKey::try_from(&issue.key.source).unwrap();
    let peer = Peer::default();
    view.update(cx, |view, cx| {
        view.repo_mut().snapshot.items = Arc::new(vec![issue]);
        view.fixture_move(peer.clone());
        view.refresh_rows();
        view.show_kanban(true, cx);
    });
    cx.run_until_parked();
    for close in 0..3 {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                let card = sheprd_orchestrator::kanban::identity::CardKey::try_from(
                    &key.parse::<ItemKey>().unwrap(),
                )
                .unwrap();
                match close {
                    0 => {
                        let drag = CardDrag {
                            board: cx.entity_id(),
                            card,
                            revision: view.kanban.revision,
                        };
                        view.kanban.drag.active_card = Some(drag.clone());
                        view.drop_card(
                            &drag,
                            &lane,
                            sheprd_control_protocol::BoardColumn::InProgress,
                            window,
                            cx,
                        )
                        .unwrap();
                        view.kanban.drag.cancel();
                    }
                    1 => {
                        view.select_move_for_test(card);
                        for key in ["m", "down", "enter"] {
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
                    }
                    _ => view
                        .propose_card(
                            &key,
                            &lane,
                            sheprd_control_protocol::BoardColumn::InProgress,
                            window,
                            cx,
                        )
                        .unwrap(),
                }
            })
        });
        wait(&view, cx, false);
        assert!(peer.writes.lock().unwrap().is_empty());
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                match close {
                    0 => view.close_move_dialog_for_test(window, cx),
                    1 => {
                        view.kanban_key(
                            &gpui::KeyDownEvent {
                                keystroke: gpui::Keystroke::parse("escape").unwrap(),
                                is_held: false,
                                prefer_character_input: false,
                            },
                            window,
                            cx,
                        );
                    }
                    _ => {
                        view.drop_move_for_test();
                    }
                }
                view.drop_move_for_test();
            })
        });
        assert!(peer.writes.lock().unwrap().is_empty());
    }
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.propose_card(
                &key,
                &lane,
                sheprd_control_protocol::BoardColumn::InProgress,
                window,
                cx,
            )
            .unwrap();
        })
    });
    wait(&view, cx, false);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.refresh_rows();
            view.request_board(cx).unwrap();
            view.confirm_move_for_test(window, cx);
        })
    });
    assert!(peer.writes.lock().unwrap().is_empty());
    cx.run_until_parked();
    // An unrelated refresh only delays confirmation until its route is ready.
    cx.update(|window, cx| view.update(cx, |view, cx| view.confirm_move_for_test(window, cx)));
    wait(&view, cx, true);
    let approved = peer.writes.lock().unwrap().clone();
    assert_eq!(approved, vec![Mutation::AddLabel(LabelId::Forgejo(10))]);
    cx.update(|window, cx| view.update(cx, |view, cx| view.confirm_move_for_test(window, cx)));
    assert_eq!(*peer.writes.lock().unwrap(), approved);
}

#[gpui::test]
fn kanban_failed_partial_and_unknown_moves_keep_observed_columns(cx: &mut gpui::TestAppContext) {
    use sheprd_control_protocol::BoardColumn;
    let (_dir, view, cx) = empty::open(cx);
    for failure in [Failure::Refused, Failure::Partial, Failure::Unknown] {
        let mut issue = move_item("test/move-dialog-outcomes");
        issue.labels = vec!["status: in-progress".into()];
        let key = issue.key.canonical();
        let lane =
            sheprd_orchestrator::kanban::identity::LaneKey::try_from(&issue.key.source).unwrap();
        let peer = Peer::default();
        *peer.outcome.lock().unwrap() = Some(failure);
        view.update(cx, |view, cx| {
            view.drop_move_for_test();
            view.repos[0].service.as_ref().unwrap().moves().clear();
            view.repo_mut().snapshot.items = Arc::new(vec![issue]);
            view.repo_mut().snapshot.moves = Arc::default();
            view.fixture_move(peer.clone());
            view.refresh_rows();
            view.show_kanban(true, cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.propose_card(&key, &lane, BoardColumn::Done, window, cx)
                    .unwrap();
            })
        });
        wait(&view, cx, false);
        cx.update(|window, cx| view.update(cx, |view, cx| view.confirm_move_for_test(window, cx)));
        wait(&view, cx, true);
        view.update(cx, |view, cx| {
            let statuses = view.repos[0].service.as_ref().unwrap().moves().snapshot();
            view.repo_mut().snapshot.moves = Arc::new(statuses);
            view.refresh_rows();
            view.request_board(cx).unwrap();
        });
        cx.run_until_parked();
        view.update(cx, |view, cx| {
            let board = view.request_board(cx).unwrap().unwrap();
            let card = board
                .pages
                .as_ref()
                .unwrap()
                .iter()
                .flat_map(|p| &p.cards)
                .find(|c| c.key == key)
                .unwrap();
            assert_eq!(
                card.column,
                if matches!(failure, Failure::Partial) {
                    BoardColumn::Open
                } else {
                    BoardColumn::InProgress
                }
            );
        });
    }
}

#[gpui::test]
fn kanban_wrong_lane_window_stale_dialog_and_unsupported_cards_never_write(
    cx: &mut gpui::TestAppContext,
) {
    use crate::orchestrator::{
        Request, Timing,
        view::{RepoSpec, repo::Repo},
    };
    use sheprd_control_protocol::BoardColumn;
    let (dir, view, cx) = empty::open(cx);
    let mut issue = move_item("test/move-dialog-refusals");
    let card = sheprd_orchestrator::kanban::identity::CardKey::try_from(&issue.key).unwrap();
    let peer = Peer::default();
    view.update(cx, |view, cx| {
        view.fixture_move(peer.clone());
        view.repo_mut().snapshot.items = Arc::new(vec![issue.clone()]);
        view.refresh_rows();
        view.show_kanban(true, cx);
    });
    cx.run_until_parked();
    let other = cx.new(|_| ());
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let mut drag = CardDrag {
                board: cx.entity_id(),
                card: card.clone(),
                revision: view.kanban.revision,
            };
            view.kanban.drag.active_card = Some(drag.clone());
            let mut source = card.item.source.clone();
            source.repository = "team/other".into();
            let lane = sheprd_orchestrator::kanban::identity::LaneKey::try_from(&source).unwrap();
            assert!(
                view.drop_card(&drag, &lane, BoardColumn::Done, window, cx)
                    .is_err()
            );
            drag.board = other.entity_id();
            assert!(
                view.drop_card(&drag, &card.lane, BoardColumn::Done, window, cx)
                    .is_err()
            );
            drag.board = cx.entity_id();
            drag.revision = view.kanban.revision.wrapping_add(1);
            assert!(matches!(
                view.drop_card(&drag, &card.lane, BoardColumn::Done, window, cx),
                Err(crate::orchestrator::view::kanban::error::Error::Model(
                    sheprd_orchestrator::kanban::Error::StaleMove
                ))
            ));
            view.kanban.drag.cancel();
            view.propose_card(
                &card.item.canonical(),
                &card.lane,
                BoardColumn::Done,
                window,
                cx,
            )
            .unwrap();
        })
    });
    wait(&view, cx, false);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.repos[0]
                .service
                .as_ref()
                .unwrap()
                .moves()
                .cancel_unconfirmed();
            view.confirm_move_for_test(window, cx);
            view.drop_move_for_test();
            issue.key.source.provider = Provider::Github;
            issue.key.source.host = "github.example.test".into();
            view.repo_mut().snapshot.items = Arc::new(vec![issue.clone()]);
            view.refresh_rows();
            view.request_board(cx).unwrap();
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let lane = sheprd_orchestrator::kanban::identity::LaneKey::try_from(&issue.key.source)
                .unwrap();
            assert!(matches!(
                view.propose_card(&issue.key.canonical(), &lane, BoardColumn::Done, window, cx),
                Err(crate::orchestrator::view::kanban::error::Error::Unsupported)
            ));
        })
    });
    assert!(peer.writes.lock().unwrap().is_empty());

    // A retained account worker cannot authorize a lane now owned by a checkout.
    issue.key.source.provider = Provider::Forgejo;
    issue.key.source.host = "forgejo.example.test".into();
    for pending in [true, false] {
        let checkout = view.update(cx, |view, cx| {
            let checkout = view.repos.remove(0);
            view.add_forgejo_account(issue.key.source.host.clone());
            view.repos.last_mut().unwrap().snapshot.items = Arc::new(vec![issue.clone()]);
            view.refresh_rows();
            view.request_board(cx).unwrap();
            checkout
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.propose_card(
                    &card.item.canonical(),
                    &card.lane,
                    BoardColumn::Done,
                    window,
                    cx,
                )
                .unwrap();
            })
        });
        wait(&view, cx, false);
        // A pending projection never authorizes a write from displayed stale rows.
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.refresh_rows();
                view.request_board(cx).unwrap();
                view.confirm_move_for_test(window, cx);
            })
        });
        cx.run_until_parked();
        assert!(peer.writes.lock().unwrap().is_empty());
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.repos.insert(0, checkout);
                view.repos[0].snapshot.sources = vec![SourceStatus {
                    key: issue.key.source.clone(),
                    state: SyncState::Synced {
                        at: Utc::now(),
                        count: 1,
                    },
                }];
                view.repos[0].snapshot.items = Arc::new(vec![issue.clone()]);
                view.refresh_rows();
                view.request_board(cx).unwrap();
                if pending {
                    view.confirm_move_for_test(window, cx);
                }
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.confirm_move_for_test(window, cx);
                let account = view.repos.last().unwrap().service.as_ref().unwrap();
                assert!(
                    account
                        .moves()
                        .snapshot()
                        .iter()
                        .any(|status| status.card == card
                            && matches!(status.phase, MovePhase::Cancelled))
                );
                view.drop_move_for_test();
            })
        });
        assert!(peer.writes.lock().unwrap().is_empty());
    }

    // A newer checkout observation can replace the card under another concrete spelling.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.propose_card(
                &card.item.canonical(),
                &card.lane,
                BoardColumn::Done,
                window,
                cx,
            )
            .unwrap();
        })
    });
    wait(&view, cx, false);
    let mut replacement = issue.clone();
    replacement.key.source.repository = "Test/Move-Dialog-Refusals".into();
    replacement.updated_at = Some(at("2026-11-12T01:00:00Z"));
    view.update(cx, |view, cx| {
        let request = Request {
            target: sheprd_client::ConnectTarget::Local,
            checkout: dir.path().join("other").to_string_lossy().into_owned(),
            workspace_id: "other".into(),
            token: None,
            data_root: Some(dir.path().join("other-data")),
            azure_store: Default::default(),
            gitlab_store: Default::default(),
            timing: Timing::default(),
        };
        let mut checkout = Repo::start(RepoSpec {
            key: "other".into(),
            host: "localhost".into(),
            request,
        });
        checkout.snapshot.items = Arc::new(vec![replacement.clone()]);
        view.repos.push(checkout);
        view.refresh_rows();
        view.request_board(cx).unwrap();
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let board = view.request_board(cx).unwrap().unwrap();
            let displayed: Vec<_> = board
                .pages
                .as_ref()
                .unwrap()
                .iter()
                .flat_map(|page| &page.cards)
                .map(|card| card.key.clone())
                .collect();
            assert!(displayed.contains(&replacement.key.canonical()));
            assert!(!displayed.contains(&card.item.canonical()));
            view.confirm_move_for_test(window, cx);
            assert!(
                view.repos[0]
                    .service
                    .as_ref()
                    .unwrap()
                    .moves()
                    .snapshot()
                    .iter()
                    .any(|status| status.card == card
                        && matches!(status.phase, MovePhase::Cancelled))
            );
        })
    });
    assert!(peer.writes.lock().unwrap().is_empty());
}
