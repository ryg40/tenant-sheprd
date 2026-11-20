//! `sheprd artifacts start` keeps its question: unlike a click on the
//! button, it starts a stopped server only after the user's yes.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::artifacts::{State, found_for_test, tests::fake_program};
use crate::browser::WebUrl;
use crate::menu::Page;
use crate::sidebar::layout_tests::fixture_window;

#[gpui::test]
fn start_on_a_stopped_server_asks_and_runs_nothing(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let page = "http://127.0.0.1:6790/browse";
    let status = format!(r#"{{"state": "stopped", "url": "{page}"}}"#);
    let program = fake_program(dir.path(), &status);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        // A focused workspace, where a start would not be refused.
        if let Some(snapshot) = &view.live.snapshot {
            let mut snapshot = (**snapshot).clone();
            snapshot.focused_workspace_id = Some("w0".into());
            view.live.snapshot = Some(std::sync::Arc::new(snapshot));
        }
        view
    });
    let read = program.clone();
    cx.update(|_, cx| found_for_test(program, cx));
    let stopped = Status {
        state: State::Stopped,
        url: Some(WebUrl::try_from(page).unwrap()),
    };
    let response = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            answer_for(&ArtifactsOp::Start {}, Ok(stopped), read, view, window, cx)
        })
    });
    cx.run_until_parked();
    assert!(matches!(response, Response::Done { .. }), "{response:?}");
    view.read_with(cx, |view, _| {
        assert_eq!(view.menu.page, Some(Page::Artifacts));
    });
    assert!(!dir.path().join("started").exists());
}
