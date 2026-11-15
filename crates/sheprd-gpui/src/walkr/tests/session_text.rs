use crate::{
    sidebar::{
        ActivityPanel,
        layout_tests::{SidebarFixture, fixture_window, full_draw},
    },
    walkr::history::{RATINGS_PAGE, parse},
};
use gpui::{AppContext as _, px, size};

const SESSION: &str = "20261101T120000Z-0123456789ab";

/// A build without a page engine shows a picked session as text.
#[gpui::test]
fn a_build_without_pages_shows_a_session_as_text(cx: &mut gpui::TestAppContext) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut view = fixture_window(window, cx);
            view.config.sidebar_style.activity_bar = true;
            view
        });
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        SidebarFixture(view)
    });
    cx.simulate_resize(size(px(1100.), px(700.)));
    let view = cx.update(|_, cx| fixture.read(cx).0.clone());
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let record = format!(
                r#"{{"schema":"walkr-compare-session","version":1,"sessionId":"{SESSION}","status":"submitted","title":"Invented","brief":"","options":[]}}"#
            );
            let detail = parse(SESSION, record.as_bytes(), RATINGS_PAGE).unwrap();
            view.walkr.sessions.test_detail(detail);
            view.show_activity_panel(ActivityPanel::Walkr, window, cx);
        });
    });
    cx.update(|window, cx| full_draw(window, cx).clear(cx));
    assert!(cx.debug_bounds("walkr-detail").is_some());
}
