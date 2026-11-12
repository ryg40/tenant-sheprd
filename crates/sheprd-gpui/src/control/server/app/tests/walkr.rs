use super::*;
use crate::sidebar::{ActivityPanel, layout_tests::fixture_window};

#[gpui::test]
fn ui_panel_walkr_refuses_an_unset_address_without_changing_the_window(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.config.sidebar_style.activity_bar = true;
        view.activity.panel = ActivityPanel::Usage;
        view
    });
    let refused = cx.cx.update(|cx| {
        ui(
            call(UiOp::Panel {
                panel: ActivityPanel::Walkr,
            }),
            cx,
        )
    });
    assert_eq!(code(&refused), Some(ErrorCode::Unsupported));
    assert_eq!(
        view.read_with(cx, |view, _| view.activity.panel),
        ActivityPanel::Usage
    );
}
