//! A click with the program reads the status first; a stopped server is
//! only asked about, and only the yes starts it. Unix only, as the fake
//! program is a shell script.
use super::*;
use crate::menu::Page;
use crate::sidebar::layout_tests::{SidebarFixture, fixture_window, full_draw};
use gpui::{AppContext as _, Modifiers, px, size};

#[gpui::test]
fn a_stopped_server_starts_only_after_the_yes(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let program = fake_program(
        dir.path(),
        r#"{"state": "stopped", "url": "http://127.0.0.1:6790/browse"}"#,
    );
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
    cx.update(|_, cx| {
        cx.set_global(Launcher {
            program: Some(program),
            job: None,
        })
    });
    let view = cx.update(|_, cx| fixture.read(cx).0.clone());
    let draw = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            window.refresh();
            full_draw(window, cx).clear(cx);
        });
    };
    // Without a URL setting, the program alone puts the button on the bar.
    draw(cx);
    let button = cx.debug_bounds("activity-artifacts").unwrap().center();
    // A click while another run holds the slot starts no second child.
    cx.update(|_, cx| assert!(claim(Job::Starting, cx)));
    cx.simulate_click(button, Modifiers::default());
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert!(view.menu.page.is_none()));
    cx.update(|_, cx| {
        assert!(busy(cx));
        release(cx);
    });

    cx.simulate_click(button, Modifiers::default());
    cx.run_until_parked();
    cx.update(|_, cx| assert!(!busy(cx)));
    view.read_with(cx, |view, _| {
        assert_eq!(view.menu.page, Some(Page::Artifacts));
    });
    assert!(!dir.path().join("started").exists());
    assert!(cx.opened_url().is_none());

    // No starts nothing.
    draw(cx);
    let no = cx.debug_bounds("artifacts-ask-no").unwrap().center();
    cx.simulate_click(no, Modifiers::default());
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert!(view.menu.page.is_none()));
    assert!(!dir.path().join("started").exists());

    // Yes starts the server, then opens the page.
    draw(cx);
    cx.simulate_click(button, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let yes = cx.debug_bounds("artifacts-ask-yes").unwrap().center();
    cx.simulate_click(yes, Modifiers::default());
    cx.run_until_parked();
    assert!(dir.path().join("started").exists());
    if !crate::browser::EMBEDDED {
        assert_eq!(
            cx.opened_url().as_deref(),
            Some("http://127.0.0.1:6790/browse")
        );
    }
}
