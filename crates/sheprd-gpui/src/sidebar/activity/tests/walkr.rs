use super::*;
use crate::{browser::WebUrl, sidebar::layout_tests::fixture_window};

#[gpui::test]
fn walkr_listing_and_restore_follow_the_configured_address(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.config.sidebar_style.activity_bar = true;
        view
    });
    view.update(cx, |view, _| {
        assert!(!view.activity_panel_listed(ActivityPanel::Walkr));
        view.activity.restore(None, &Default::default());
        assert!(view.activity_section().is_none());
        view.activity
            .restore(Some(ActivityPanel::Walkr), &Default::default());
        assert_eq!(view.activity_panel(), ActivityPanel::Workspaces);
        view.config.walkr.url = Some(WebUrl::try_from("https://walkr.example.test").unwrap());
        assert!(view.activity_panel_listed(ActivityPanel::Walkr));
        assert_eq!(view.activity_section(), Some(ActivityPanel::Walkr));
        view.config.walkr.url = None;
        assert!(!view.activity_panel_listed(ActivityPanel::Walkr));
        assert_eq!(view.activity_panel(), ActivityPanel::Workspaces);
        assert!(view.activity_section().is_none());
    });
}

fn run_address() -> crate::walkr::run::Parsed {
    crate::walkr::run::parse(&sheprd_control_protocol::protocol::walkr::RunAddress::new(
        "http://127.0.0.1:4100/_walkr/compare/g/INVENTEDSECRET#20261101T120000Z-0123456789ab"
            .to_owned(),
    ))
    .unwrap()
}

#[gpui::test]
fn walkr_is_listed_for_a_run_a_saved_session_or_the_setting(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.config.sidebar_style.activity_bar = true;
        view
    });
    view.update(cx, |view, _| {
        // None of the three.
        assert!(!view.activity_panel_listed(ActivityPanel::Walkr));
        // A saved session of this computer.
        view.walkr.history_checked(true);
        assert!(view.activity_panel_listed(ActivityPanel::Walkr));
        view.walkr.history_checked(false);
        assert!(!view.activity_panel_listed(ActivityPanel::Walkr));
        // The setting.
        view.config.walkr.url = Some(WebUrl::try_from("https://walkr.example.test").unwrap());
        assert!(view.activity_panel_listed(ActivityPanel::Walkr));
        view.config.walkr.url = None;
        // A live run.
        let pane = crate::walkr::run::Pane::new("p1", None);
        let now = std::time::Instant::now();
        view.walkr
            .connect(run_address(), pane.clone(), now)
            .unwrap();
        assert!(view.activity_panel_listed(ActivityPanel::Walkr));
        // A run that ends with no page is dropped at once.
        view.walkr.disconnect(&pane);
        assert!(view.walkr.run().is_none());
        assert!(!view.activity_panel_listed(ActivityPanel::Walkr));
    });
}

/// An ended run whose page is kept counts as a run; Close drops it, and a
/// window that showed Walkr shows Workspaces.
#[cfg(embedded_pages)]
#[gpui::test]
fn an_ended_run_with_its_page_is_listed_until_close(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.config.sidebar_style.activity_bar = true;
        view
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let pane = crate::walkr::run::Pane::new("p1", None);
            view.connect_walkr(run_address(), pane.clone(), true, window, cx)
                .unwrap();
            assert_eq!(view.activity_section(), Some(ActivityPanel::Walkr));
            view.walkr.page_for = view.walkr.source(&view.config.walkr);
            view.walkr.disconnect(&pane);
            assert!(view.walkr.run().is_some_and(|run| !run.live()));
            assert!(view.activity_panel_listed(ActivityPanel::Walkr));
            assert_eq!(view.activity_section(), Some(ActivityPanel::Walkr));
            view.dismiss_walkr_page();
            assert!(view.walkr.run().is_none());
            assert!(!view.activity_panel_listed(ActivityPanel::Walkr));
            assert_eq!(view.activity_panel(), ActivityPanel::Workspaces);
            assert!(view.activity_section().is_none());
        });
    });
}
