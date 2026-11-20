use super::*;
use crate::{
    browser::Store,
    sidebar::layout_tests::{full_draw, snapshot},
};

/// The engine's word that a page stopped waits with the page, so a window
/// whose report queue is full still shows the tab's reload when it next
/// runs. The engine is the tests' inert one: no page runs.
#[gpui::test]
fn a_stop_reaches_its_tab_through_a_full_report_queue(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        let mut shown = snapshot(40);
        shown.focused_workspace_id = Some("w0".into());
        shown.focused_tab_id = Some("t0".into());
        view.live.snapshot = Some(Arc::new(shown));
        view
    });
    cx.update(|window, cx| {
        let scope = crate::browser::scope(&view.read(cx).endpoints[0]);
        let location = Location::Web {
            url: WebUrl::try_from("https://site.test/app").unwrap(),
        };
        let id = Store::update(cx, |store| store.open(scope, "w0", Some(location), None)).unwrap();
        view.update(cx, |view, cx| {
            view.show_browser_tab(id, window, cx);
            let reports = view.browser.pages.reports.clone();
            while reports
                .try_send(Event::Title(Source::Tab(id), String::new()))
                .is_ok()
            {}
            // What the engine does once it finds the page's thread gone.
            view.browser.pages.pages[&id].mailbox().stopped.set();
            view.poll_browser(window, cx);
        });
    });
    cx.update(|window, cx| full_draw(window, cx).clear(cx));
    assert!(cx.debug_bounds("browser-stopped-reload").is_some());
}
