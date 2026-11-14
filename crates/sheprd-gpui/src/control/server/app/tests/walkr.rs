use super::*;
use crate::sidebar::{ActivityPanel, layout_tests::fixture_window};

#[gpui::test]
fn ui_panel_walkr_shows_the_section_with_no_address(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.config.sidebar_style.activity_bar = true;
        view.activity.panel = ActivityPanel::Usage;
        view
    });
    let shown = cx.cx.update(|cx| {
        ui(
            call(UiOp::Panel {
                panel: ActivityPanel::Walkr,
            }),
            cx,
        )
    });
    assert_eq!(code(&shown), None);
    assert_eq!(
        view.read_with(cx, |view, _| view.activity.panel),
        ActivityPanel::Walkr
    );
}
