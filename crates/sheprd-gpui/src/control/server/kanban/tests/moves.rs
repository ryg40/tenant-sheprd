use super::*;

#[gpui::test]
fn kanban_move_command_refuses_unknown_windows_disabled_board_and_existing_modal(
    cx: &mut gpui::TestAppContext,
) {
    use crate::{control::protocol::Caller, sidebar::layout_tests::fixture_window};
    let (view, cx) = cx.add_window_view(fixture_window);
    let proposal = |caller| Call {
        caller,
        op: OrchestratorOp::BoardMove {
            item: "forgejo:forgejo.example.test:team/app:7".into(),
            lane: "forgejo:forgejo.example.test:team/app".into(),
            to: wire::BoardColumn::Done,
        },
    };
    cx.cx.update(|cx| {
        assert!(matches!(
            serve(
                proposal(Caller {
                    window: Some(u64::MAX),
                    ..Caller::default()
                }),
                cx
            ),
            Response::Error { .. }
        ));
    });
    view.update(cx, |view, _| view.config.sidebar_style.activity_bar = false);
    cx.cx.update(|cx| {
        assert!(matches!(
            serve(proposal(Caller::default()), cx),
            Response::Error {
                code: ErrorCode::Unsupported,
                ..
            }
        ));
    });
    view.update(cx, |view, _| {
        view.config.sidebar_style.activity_bar = true;
        view.menu.page = Some(crate::menu::Page::Menu);
    });
    cx.cx.update(|cx| {
        assert!(matches!(
            serve(proposal(Caller::default()), cx),
            Response::Error {
                code: ErrorCode::Busy,
                ..
            }
        ));
    });
}
