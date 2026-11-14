use super::*;
use crate::sidebar::layout_tests::fixture_window;

#[gpui::test]
fn walkr_is_listed_and_restored_whatever_its_sources(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.config.sidebar_style.activity_bar = true;
        view
    });
    view.update(cx, |view, _| {
        assert!(view.activity_panel_listed(ActivityPanel::Walkr));
        view.activity
            .restore(Some(ActivityPanel::Walkr), &Default::default());
        assert_eq!(view.activity_section(), Some(ActivityPanel::Walkr));
    });
}

#[cfg(embedded_pages)]
fn run_address() -> crate::walkr::run::Parsed {
    crate::walkr::run::parse(&sheprd_control_protocol::protocol::walkr::RunAddress::new(
        "http://127.0.0.1:4100/_walkr/compare/g/INVENTEDSECRET#20261101T120000Z-0123456789ab"
            .to_owned(),
    ))
    .unwrap()
}

/// Close of an ended run's page keeps the section.
#[cfg(embedded_pages)]
#[gpui::test]
fn close_of_an_ended_runs_page_keeps_the_section(cx: &mut gpui::TestAppContext) {
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
            view.dismiss_walkr_page();
            assert_eq!(view.activity_section(), Some(ActivityPanel::Walkr));
        });
    });
}
