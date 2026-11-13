mod moves;

use super::*;

#[test]
fn kanban_cursors_refuse_stale_cross_window_and_oversized_requests() {
    let pages = vec![wire::BoardPage {
        revision: 9,
        columns: wire::BoardColumn::ALL.to_vec(),
        roster: Default::default(),
        coverage: Default::default(),
        lanes: Vec::new(),
        cards: Vec::new(),
        next_cursor: None,
        expanded_done_history: false,
    }];
    assert!(matches!(
        page(
            &pages,
            1,
            Some(Cursor {
                window: 2,
                revision: 9,
                offset: 0
            })
        ),
        Response::Orchestrator {
            reply: BoardReply::RestartRequired
        }
    ));
    assert!(matches!(
        page(
            &pages,
            1,
            Some(Cursor {
                window: 1,
                revision: 8,
                offset: 0
            })
        ),
        Response::Orchestrator {
            reply: BoardReply::RestartRequired
        }
    ));
    assert!(matches!(
        page(
            &pages,
            1,
            Some(Cursor {
                window: 1,
                revision: 9,
                offset: usize::MAX
            })
        ),
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(parse_cursor(&"f".repeat(wire::MAX_CURSOR_BYTES + 1)).is_err());
    assert!(parse_cursor("1.9.0.extra").is_err());
}

#[gpui::test]
fn kanban_reads_and_panel_refuse_wrong_window_without_changing_the_shown_panel(
    cx: &mut gpui::TestAppContext,
) {
    use crate::{
        control::protocol::{Caller, app::UiOp},
        sidebar::{ActivityPanel, layout_tests::fixture_window},
    };
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.config.sidebar_style.activity_bar = true;
        view
    });
    let before = view.read_with(cx, |view, _| view.activity.panel);
    let caller = Caller {
        workspace_id: Some("unshown-session".into()),
        ..Caller::default()
    };
    cx.cx.update(|cx| {
        for response in [
            serve(
                Call {
                    caller: caller.clone(),
                    op: OrchestratorOp::Board { cursor: None },
                },
                cx,
            ),
            super::super::app::ui(
                Call {
                    caller,
                    op: UiOp::Panel {
                        panel: ActivityPanel::Kanban,
                    },
                },
                cx,
            ),
        ] {
            assert!(matches!(
                response,
                Response::Error {
                    code: ErrorCode::WorkspaceNotFound,
                    ..
                }
            ));
        }
    });
    assert_eq!(view.read_with(cx, |view, _| view.activity.panel), before);
    cx.cx.update(|cx| {
        assert!(matches!(
            serve(
                Call {
                    caller: Caller::default(),
                    op: OrchestratorOp::Board { cursor: None }
                },
                cx
            ),
            Response::Orchestrator { .. }
        ));
    });
    assert_eq!(view.read_with(cx, |view, _| view.activity.panel), before);
    cx.run_until_parked();
    cx.cx.update(|cx| {
        let read = serve(
            Call {
                caller: Caller::default(),
                op: OrchestratorOp::Board { cursor: None },
            },
            cx,
        );
        assert!(matches!(
            read,
            Response::Orchestrator {
                reply: BoardReply::Page(_)
            }
        ));
        let shown = super::super::app::ui(
            Call {
                caller: Caller::default(),
                op: UiOp::Panel {
                    panel: ActivityPanel::Kanban,
                },
            },
            cx,
        );
        assert!(matches!(shown, Response::Done { .. }));
    });
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.activity.panel),
        ActivityPanel::Kanban
    );
}
