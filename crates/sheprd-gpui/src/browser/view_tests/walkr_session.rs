use super::*;
use crate::{
    browser::{TabId, reports::Event},
    sidebar::ActivityPanel,
    walkr::history::{RATINGS_PAGE, parse},
};

const SESSION: &str = "20261101T120000Z-0123456789ab";

/// A page failure gives the text view back, and no page is made again for
/// that session.
#[gpui::test]
fn a_failed_session_page_leaves_the_text_view(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.config.sidebar_style.activity_bar = true;
            let record = format!(
                r#"{{"schema":"walkr-compare-session","version":1,"sessionId":"{SESSION}","status":"submitted","title":"Invented","brief":"","options":[]}}"#
            );
            let detail = parse(SESSION, record.as_bytes(), RATINGS_PAGE).unwrap();
            view.walkr.sessions.test_detail(detail);
            view.show_activity_panel(ActivityPanel::Walkr, window, cx);
            view.poll_walkr_session_page(window, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.browser.pages.contains(TabId::WALKR_SESSION));
            // Text until the page loaded its document.
            assert!(!view.shows_walkr_session_page());
            let address = view.walkr_session_address().unwrap();
            view.apply_page_event(Event::Loaded(TabId::WALKR_SESSION, address), window, cx);
            assert!(view.shows_walkr_session_page());
            view.apply_page_event(
                Event::Failed(TabId::WALKR_SESSION, crate::browser::engine::Error::Stopped),
                window,
                cx,
            );
            view.poll_walkr_session_page(window, cx);
            assert!(!view.shows_walkr_session_page());
            assert!(view.walkr.detail_shown());
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.poll_walkr_session_page(window, cx);
            assert!(!view.browser.pages.contains(TabId::WALKR_SESSION));
        });
    });
}
