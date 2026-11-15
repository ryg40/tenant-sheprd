#![allow(clippy::unwrap_used)]
use super::*;
use crate::{
    browser::{WebUrl, reports::report},
    sidebar::{ActivityPanel, layout_tests::fixture_window},
    walkr::Connectivity,
};

#[gpui::test]
fn retired_walkr_reports_leave_the_replacement_state_and_page_unchanged(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.config.sidebar_style.activity_bar = true;
            view.config.walkr.url = Some(WebUrl::try_from("https://walkr.example.test").unwrap());
            view.walkr
                .test_connectivity(&view.config.walkr, Connectivity::Reachable);
            view.show_activity_panel(ActivityPanel::Walkr, window, cx);
            view.poll_walkr_page(std::time::Instant::now(), window, cx);

            // Both a URL edit and a retry retire the old instance.
            for change_url in [true, false] {
                let reports = view.browser.pages.reports.clone();
                let retired = Source::Walkr(view.browser.pages.walkr.unwrap());
                if change_url {
                    view.config.walkr.url =
                        Some(WebUrl::try_from("https://walkr.example.test/review").unwrap());
                    view.poll_walkr_page(std::time::Instant::now(), window, cx);
                } else {
                    view.close_walkr_page();
                }
                view.walkr
                    .test_connectivity(&view.config.walkr, Connectivity::Reachable);
                view.poll_walkr_page(std::time::Instant::now(), window, cx);
                let replacement = view.browser.pages.pages[&TabId::WALKR].clone();
                let current = Source::Walkr(view.browser.pages.walkr.unwrap());
                report(
                    &reports,
                    Event::Loaded(current, "https://walkr.example.test/review".into()),
                );
                while let Some(event) = view.browser.pages.next_event() {
                    view.apply_page_event(event, window, cx);
                }
                assert_eq!(view.walkr.connectivity(), Connectivity::Ready);
                for event in [
                    Event::Loaded(retired, "https://other.example.test/flow".into()),
                    Event::Failed(retired, super::super::engine::Error::Stopped),
                ] {
                    report(&reports, event);
                    while let Some(event) = view.browser.pages.next_event() {
                        view.apply_page_event(event, window, cx);
                    }
                    assert_eq!(view.walkr.connectivity(), Connectivity::Ready);
                    assert!(view.walkr.page_error.is_none());
                    assert!(Rc::ptr_eq(
                        &replacement,
                        &view.browser.pages.pages[&TabId::WALKR],
                    ));
                }
            }
        });
    });
}

const SECRET_PARTS: [&str; 4] = ["INVENTED", "_walkr", "compare", "20261101"];

fn run_address(key: &str) -> String {
    format!("http://127.0.0.1:4100/_walkr/compare/g/{key}#20261101T120000Z-0123456789ab")
}

fn parsed(key: &str) -> crate::walkr::run::Parsed {
    let address = sheprd_control_protocol::protocol::walkr::RunAddress::new(run_address(key));
    crate::walkr::run::parse(&address).unwrap()
}

fn pane(id: &str) -> crate::walkr::run::Pane {
    crate::walkr::run::Pane::new(id, None)
}

/// Connects pane `p1` to a run and makes its page, as a tick does once the
/// probe answered.
fn run_page(view: &mut SheprdWindow, window: &mut Window, cx: &mut Context<SheprdWindow>) {
    view.config.sidebar_style.activity_bar = true;
    view.connect_walkr(parsed("INVENTEDSECRET"), pane("p1"), true, window, cx)
        .unwrap();
    view.walkr
        .test_connectivity(&view.config.walkr, Connectivity::Reachable);
    view.poll_walkr_page(std::time::Instant::now(), window, cx);
    assert!(view.browser.pages.contains(TabId::WALKR));
}

fn tabs(cx: &gpui::App) -> usize {
    cx.try_global::<crate::browser::Store>()
        .map_or(0, |store| store.tabs().count())
}

#[gpui::test]
fn the_page_of_a_run_and_its_reports_show_host_and_port_at_most(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            run_page(view, window, cx);
            let assert_no_secret = |shown: String| {
                for hidden in SECRET_PARTS {
                    assert!(!shown.contains(hidden), "{shown}");
                }
            };
            // What the page was made from stands in for a tab: the section
            // has none, in the store or outside it.
            let made = format!("{:?}", view.walkr.run().unwrap().address());
            assert!(made.contains("127.0.0.1:4100"), "{made}");
            assert_no_secret(made);
            assert_eq!(tabs(cx), 0);
            let source = Source::Walkr(view.browser.pages.walkr.unwrap());
            let address = run_address("INVENTEDSECRET");
            for queued in [
                Event::Loaded(source, address.clone()),
                Event::Title(source, address.clone()),
                Event::NewWindow(source, address.clone()),
            ] {
                assert_no_secret(format!("{queued:?} {queued:#?}"));
                report(&view.browser.pages.reports, queued);
                let event = view.browser.pages.next_event().unwrap();
                assert_no_secret(format!("{event:?} {event:#?}"));
                view.apply_page_event(event, window, cx);
            }
            assert_eq!(view.walkr.connectivity(), Connectivity::Ready);
            assert_no_secret(format!("{:?}", view.walkr));
            assert_eq!(tabs(cx), 0);
        });
    });
}

#[gpui::test]
fn a_report_of_the_load_before_a_changed_address_changes_no_state_and_no_page(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            run_page(view, window, cx);
            let reports = view.browser.pages.reports.clone();
            let first = view.browser.pages.pages[&TabId::WALKR].clone();
            let retired = Source::Walkr(view.browser.pages.walkr.unwrap());
            // Another pane with the same address joins the page there is.
            view.connect_walkr(parsed("INVENTEDSECRET"), pane("p2"), false, window, cx)
                .unwrap();
            assert!(Rc::ptr_eq(&first, &view.browser.pages.pages[&TabId::WALKR]));
            // Another address of the same server gets a page of its own.
            view.connect_walkr(parsed("INVENTEDOTHER"), pane("p2"), false, window, cx)
                .unwrap();
            let replacement = view.browser.pages.pages[&TabId::WALKR].clone();
            assert!(!Rc::ptr_eq(&first, &replacement));
            assert_eq!(view.walkr.connectivity(), Connectivity::Reachable);
            for stale in [
                Event::Loaded(retired, run_address("INVENTEDSECRET")),
                Event::Failed(retired, super::super::engine::Error::Stopped),
                Event::NewWindow(retired, "https://other.example.test/".into()),
            ] {
                report(&reports, stale);
                while let Some(event) = view.browser.pages.next_event() {
                    view.apply_page_event(event, window, cx);
                }
                assert_eq!(view.walkr.connectivity(), Connectivity::Reachable);
                assert!(view.walkr.page_error.is_none());
                assert!(Rc::ptr_eq(
                    &replacement,
                    &view.browser.pages.pages[&TabId::WALKR],
                ));
                assert_eq!(tabs(cx), 0);
            }
            // The load of the new address counts.
            let current = Source::Walkr(view.browser.pages.walkr.unwrap());
            report(
                &reports,
                Event::Loaded(current, run_address("INVENTEDOTHER")),
            );
            while let Some(event) = view.browser.pages.next_event() {
                view.apply_page_event(event, window, cx);
            }
            assert_eq!(view.walkr.connectivity(), Connectivity::Ready);
        });
    });
}

/// The page of a saved Walkr session, one instance at a time.
mod walkr_session;
