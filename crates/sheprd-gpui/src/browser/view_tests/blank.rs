use super::*;
use std::time::{Duration, Instant};

/// A site's page that loads with nothing in it gets the hint, and the next
/// load of the tab takes it away. The engine is the tests' inert one: the
/// window's question is queued and the test answers for the page.
#[gpui::test]
fn an_empty_page_shows_a_hint_until_the_tab_loads_again(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    let id = cx.update(|window, cx| {
        let scope = scope(&view.read(cx).endpoints[0]);
        let location = url("https://site.test/empty");
        let id = Store::update(cx, |store| store.open(scope, "w0", Some(location), None)).unwrap();
        view.update(cx, |view, cx| view.show_browser_tab(id, window, cx));
        id
    });
    draw(cx);
    assert!(cx.debug_bounds("browser-blank").is_none());

    let start = Instant::now();
    let later = start + Duration::from_secs(1);
    view.update(cx, |view, cx| {
        view.browser.blank.loaded(id, start);
        // Too early to ask, then asked.
        view.poll_blank_pages(start, cx);
        assert!(!view.browser.blank.answer(id, "true"));
        view.poll_blank_pages(later, cx);
        assert!(view.browser.blank.answer(id, "true"));
        view.poll_blank_pages(later, cx);
    });
    draw(cx);
    assert!(cx.debug_bounds("browser-blank").is_some());
    assert!(cx.debug_bounds("browser-blank-reload").is_some());

    view.update(cx, |view, _| view.browser.blank.loaded(id, later));
    draw(cx);
    assert!(cx.debug_bounds("browser-blank").is_none());
}

/// A tab without a page in this window, and a page that is not a site's,
/// are never asked and never marked.
#[gpui::test]
fn a_local_page_gets_no_hint(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    let path = std::env::temp_dir().join("mockups").join("index.html");
    let file = crate::browser::LocalFile::new(&path, None).unwrap();
    let id = cx.update(|window, cx| {
        let scope = scope(&view.read(cx).endpoints[0]);
        let location = Location::Local { file };
        let id = Store::update(cx, |store| store.open(scope, "w0", Some(location), None)).unwrap();
        view.update(cx, |view, cx| view.show_browser_tab(id, window, cx));
        id
    });
    let start = Instant::now();
    view.update(cx, |view, cx| {
        view.browser.blank.loaded(id, start);
        view.poll_blank_pages(start + Duration::from_secs(1), cx);
        assert!(!view.browser.blank.answer(id, "true"));
        view.poll_blank_pages(start + Duration::from_secs(5), cx);
        assert!(!view.browser.blank.shows(id));
    });
}

/// The hint never sends a one-time code again on its own: at an address
/// with a query, as a sign-in's last address has, its action opens the
/// address the tab was asked for on that site instead of a reload.
#[gpui::test]
fn the_hint_at_an_address_with_a_code_opens_the_address_asked_for(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    let id = cx.update(|window, cx| {
        let scope = scope(&view.read(cx).endpoints[0]);
        let asked = url("https://site.test/app");
        let id = Store::update(cx, |store| store.open(scope, "w0", Some(asked), None)).unwrap();
        // The site sends the tab to its sign-in, which ends on an address
        // of the site with a code in it.
        for step in [
            "https://auth.test/flow",
            "https://site.test/callback?code=once",
        ] {
            Store::update(cx, |store| store.visited(id, Some(url(step)), None));
        }
        view.update(cx, |view, cx| view.show_browser_tab(id, window, cx));
        id
    });
    let start = Instant::now();
    let later = start + Duration::from_secs(1);
    view.update(cx, |view, cx| {
        view.browser.blank.loaded(id, start);
        view.poll_blank_pages(later, cx);
        assert!(view.browser.blank.answer(id, "true"));
        view.poll_blank_pages(later, cx);
    });
    draw(cx);
    assert!(cx.debug_bounds("browser-blank-reload").is_none());
    let open = cx.debug_bounds("browser-blank-open").unwrap().center();
    cx.simulate_click(open, gpui::Modifiers::default());
    let address = cx.update(|_, cx| {
        let tab = crate::browser::view::store(cx).unwrap().get(id).unwrap();
        match &tab.location {
            Some(Location::Web { url }) => url.as_str().to_owned(),
            other => panic!("not a web address: {other:?}"),
        }
    });
    assert_eq!(address, "https://site.test/app");
    draw(cx);
    assert!(cx.debug_bounds("browser-blank").is_none());
}
