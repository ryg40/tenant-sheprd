//! Without the `artifacts` program, the Artifacts button is on the bar only
//! while `[artifacts] url` is set.
use crate::browser::WebUrl;
use crate::sidebar::layout_tests::{SidebarFixture, fixture_window, full_draw};
use gpui::{AppContext as _, px, size};

#[gpui::test]
fn artifacts_button_shows_only_with_a_url(cx: &mut gpui::TestAppContext) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut view = fixture_window(window, cx);
            view.config.sidebar_style.activity_bar = true;
            view
        });
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        SidebarFixture(view)
    });
    cx.simulate_resize(size(px(1000.), px(700.)));
    let view = cx.update(|_, cx| fixture.read(cx).0.clone());
    let draw = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            window.refresh();
            full_draw(window, cx).clear(cx);
        });
    };
    draw(cx);
    assert!(cx.debug_bounds("activity-settings").is_some());
    assert!(cx.debug_bounds("activity-artifacts").is_none());

    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.config.artifacts.url = Some(WebUrl::try_from("http://127.0.0.1:8080/").unwrap());
            cx.notify();
        })
    });
    draw(cx);
    assert!(cx.debug_bounds("activity-artifacts").is_some());
}
