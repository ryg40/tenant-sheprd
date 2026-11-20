use super::*;
use crate::browser::{
    engine::{Error, inert},
    reports::Event,
};

/// The report that a tab's page stopped shows the tab's line with a
/// reload, and the reload opens the page again at the address the tab
/// showed. The engine is the tests' inert one: no page runs, and none has
/// to panic.
#[gpui::test]
fn a_stopped_page_shows_a_reload_that_opens_its_address_again(cx: &mut gpui::TestAppContext) {
    let address = "https://site.test/app";
    let (view, cx) = window(cx);
    let id = cx.update(|window, cx| {
        let scope = scope(&view.read(cx).endpoints[0]);
        let id = Store::update(cx, |store| {
            store.open(scope, "w0", Some(url(address)), None)
        })
        .unwrap();
        view.update(cx, |view, cx| view.show_browser_tab(id, window, cx));
        id
    });
    draw(cx);
    // Clear the recorder: only the reload's address counts.
    inert::opened();
    assert!(cx.debug_bounds("browser-stopped-reload").is_none());

    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.apply_page_event(Event::Failed(id, Error::ScriptEnded), window, cx);
        });
    });
    draw(cx);
    assert!(cx.debug_bounds("browser-placeholder").is_some());
    let reload = cx.debug_bounds("browser-stopped-reload").unwrap().center();

    cx.simulate_click(reload, gpui::Modifiers::default());
    draw(cx);
    assert_eq!(inert::opened(), [address]);
    assert!(cx.debug_bounds("browser-stopped-reload").is_none());
    assert!(cx.debug_bounds("browser-placeholder").is_none());
}
