use super::*;
use crate::walkr::history::{RATINGS_PAGE, parse};

/// A picked saved session whose record has nothing in it but `id`.
fn pick(view: &mut SheprdWindow, id: &str) {
    let record = format!(
        r#"{{"schema":"walkr-compare-session","version":1,"sessionId":"{id}","status":"submitted","title":"Invented","brief":"","options":[]}}"#
    );
    let detail = parse(id, record.as_bytes(), RATINGS_PAGE).unwrap();
    view.walkr.sessions.test_detail(detail);
}

/// A failure the page of the last pick reports late leaves the page of the
/// new pick shown.
#[gpui::test]
fn a_retired_session_page_failure_leaves_the_current_page(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.config.sidebar_style.activity_bar = true;
            pick(view, "20261101T120000Z-0123456789ab");
            view.show_activity_panel(ActivityPanel::Walkr, window, cx);
            view.poll_walkr_session_page(window, cx);
        });
    });
    cx.run_until_parked();
    let retired = cx.update(|_, cx| {
        view.update(cx, |view, _| {
            let retired = Source::WalkrSession(view.browser.pages.walkr_session.unwrap());
            pick(view, "20261102T120000Z-0123456789ab");
            retired
        })
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| view.poll_walkr_session_page(window, cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let current = Source::WalkrSession(view.browser.pages.walkr_session.unwrap());
            assert_ne!(current, retired);
            let replacement = view.browser.pages.pages[&TabId::WALKR_SESSION].clone();
            let reports = view.browser.pages.reports.clone();
            let address = view.walkr_session_address().unwrap();
            report(&reports, Event::Loaded(current, address));
            report(
                &reports,
                Event::Failed(retired, crate::browser::engine::Error::Stopped),
            );
            while let Some(event) = view.browser.pages.next_event() {
                view.apply_page_event(event, window, cx);
            }
            view.poll_walkr_session_page(window, cx);
            assert!(view.shows_walkr_session_page());
            assert!(Rc::ptr_eq(
                &replacement,
                &view.browser.pages.pages[&TabId::WALKR_SESSION],
            ));
        });
    });
}
