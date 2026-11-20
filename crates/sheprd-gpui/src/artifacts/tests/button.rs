//! A click with the program reads the status first; a stopped local server
//! then starts without a question and its page opens. Unix only, as the
//! fake program is a shell script.
use super::*;
use gpui::Modifiers;

#[gpui::test]
fn a_click_starts_a_stopped_server_at_once_and_opens_its_page(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let page = url(&format!("http://127.0.0.1:{}/browse", free_port()));
    let status = format!(r#"{{"state": "stopped", "url": "{}"}}"#, page.as_str());
    let program = fake_program(dir.path(), &status);
    let (view, cx, button) = clicking(cx, program, Default::default());
    let starts = || {
        std::fs::read_to_string(dir.path().join("started"))
            .map_or(0, |started| started.lines().count())
    };
    // A click while another run holds the slot starts no second child.
    cx.update(|_, cx| assert!(claim(Job::Starting, cx)));
    cx.simulate_click(button, Modifiers::default());
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert!(view.menu.page.is_none()));
    assert_eq!(starts(), 0);
    cx.update(|_, cx| {
        assert!(busy(cx));
        release(cx);
    });

    // One click starts the stopped server, asks nothing, and opens its page.
    cx.simulate_click(button, Modifiers::default());
    cx.run_until_parked();
    cx.update(|_, cx| assert!(!busy(cx)));
    view.read_with(cx, |view, _| assert!(view.menu.page.is_none()));
    assert_eq!(starts(), 1);
    // The page's tab is the one the window shows, not only one it has.
    assert!(shows(&view, cx, &page));
}
