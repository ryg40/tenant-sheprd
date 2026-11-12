#[cfg(embedded_pages)]
use super::*;
use crate::browser::TabId;
#[cfg(embedded_pages)]
use crate::{browser::reports::Event, sidebar::ActivityPanel, walkr::Connectivity};

#[test]
fn walkr_has_its_own_panel_id() {
    assert!(TabId::WALKR.is_panel());
    assert_ne!(TabId::WALKR, TabId::panel(1));
}

#[cfg(embedded_pages)]
fn configure(view: &mut SheprdWindow) {
    view.config.sidebar_style.activity_bar = true;
    view.config.walkr.url = Some(WebUrl::try_from("https://walkr.example.test").unwrap());
    view.walkr
        .test_connectivity(&view.config.walkr, Connectivity::Reachable);
}

#[cfg(embedded_pages)]
#[gpui::test]
fn walkr_loads_do_not_change_tabs_or_follow_other_pages(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            configure(view);
            view.walkr.page_for = view.walkr.source(&view.config.walkr);
            let location = url("https://tab.example.test");
            let id = Store::update(cx, |store| {
                store.open(
                    scope(&view.endpoints[0]),
                    "w0",
                    Some(location.clone()),
                    None,
                )
            })
            .unwrap();
            view.apply_page_event(
                Event::Loaded(
                    TabId::WALKR,
                    "https://other.example.test/flow?code=synthetic".into(),
                ),
                window,
                cx,
            );
            assert_eq!(view.walkr.connectivity(), Connectivity::Reachable);
            assert_eq!(
                cx.global::<Store>().get(id).unwrap().location,
                Some(location)
            );
            assert!(cx.global::<Store>().get(TabId::WALKR).is_none());
            for other in [id, TabId::panel(1)] {
                view.apply_page_event(
                    Event::Loaded(other, "https://walkr.example.test/review".into()),
                    window,
                    cx,
                );
                assert_eq!(view.walkr.connectivity(), Connectivity::Reachable);
            }
            view.apply_page_event(
                Event::Loaded(TabId::WALKR, "https://walkr.example.test/review".into()),
                window,
                cx,
            );
            assert_eq!(view.walkr.connectivity(), Connectivity::Ready);
        });
    });
}

// A headless GPUI window has no native handle for wry. Servo's inert test
// engine lets the same page lifecycle run without starting a browser.
#[cfg(servo_pages)]
#[gpui::test]
fn walkr_waits_for_a_shown_reachable_section_and_closes_on_url_change(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = window(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            configure(view);
            view.poll_walkr_page(std::time::Instant::now(), window, cx);
            assert!(!view.browser.pages.contains(TabId::WALKR));
            view.show_activity_panel(ActivityPanel::Walkr, window, cx);
            for state in [
                Connectivity::Checking,
                Connectivity::Unreachable {
                    cause: crate::walkr::Unreachable::Connect,
                },
                Connectivity::NetworkRefused,
                Connectivity::Down { status: 502 },
            ] {
                view.walkr.test_connectivity(&view.config.walkr, state);
                view.poll_walkr_page(std::time::Instant::now(), window, cx);
                assert!(!view.browser.pages.contains(TabId::WALKR));
            }
            view.walkr
                .test_connectivity(&view.config.walkr, Connectivity::Reachable);
            view.poll_walkr_page(std::time::Instant::now(), window, cx);
            assert!(view.browser.pages.contains(TabId::WALKR));
            view.show_activity_panel(ActivityPanel::Workspaces, window, cx);
            let id = Store::update(cx, |store| {
                store.open(scope(&view.endpoints[0]), "w0", None, None)
            })
            .unwrap();
            view.close_browser_tab(id, window, cx);
            view.poll_browser(window, cx);
            assert!(view.browser.pages.contains(TabId::WALKR));
            view.apply_page_event(
                Event::Loaded(TabId::WALKR, "https://walkr.example.test/review".into()),
                window,
                cx,
            );
            view.show_activity_panel(ActivityPanel::Walkr, window, cx);
            view.poll_walkr_page(std::time::Instant::now(), window, cx);
            assert_eq!(view.walkr.connectivity(), Connectivity::Ready);
            view.config.walkr.url = Some(WebUrl::try_from("https://next.example.test").unwrap());
            view.poll_walkr_page(std::time::Instant::now(), window, cx);
            assert!(!view.browser.pages.contains(TabId::WALKR));
            assert_eq!(view.walkr.connectivity(), Connectivity::Reachable);
        });
    });
}

#[cfg(servo_pages)]
#[gpui::test]
fn walkr_page_failure_stays_in_the_section_until_retry(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            configure(view);
            view.show_activity_panel(ActivityPanel::Walkr, window, cx);
            view.poll_walkr_page(std::time::Instant::now(), window, cx);
            view.apply_page_event(
                Event::Failed(TabId::WALKR, crate::browser::engine::Error::Stopped),
                window,
                cx,
            );
            assert!(view.walkr.page_error.is_some());
            assert!(!view.browser.failed.contains_key(&TabId::WALKR));
            view.poll_walkr_page(std::time::Instant::now(), window, cx);
            assert!(!view.browser.pages.contains(TabId::WALKR));
        });
    });
}

// A saved tab file and a saved group layout are separate files. The store
// refuses a tab in the panel range; the layout that named it must not then
// close the section's own page.
#[cfg(servo_pages)]
#[gpui::test]
fn a_saved_layout_that_names_a_refused_tab_leaves_the_walkr_page(cx: &mut gpui::TestAppContext) {
    use crate::browser::groups::Pick;
    let (view, cx) = window(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let scope = scope(&view.endpoints[0]);
            let tab = |id: u64| {
                serde_json::json!({
                    "id": id,
                    "scope": scope,
                    "workspace_id": "w0",
                    "location": {"kind": "web", "url": "https://tab.example.test/"},
                    "title": "",
                })
            };
            let refused = TabId::WALKR.number();
            let tabs = serde_json::json!({ "tabs": [tab(1), tab(refused), tab(2)] });
            cx.set_global(Store::restored(&serde_json::to_vec(&tabs).unwrap()).unwrap());
            let page = |id: u64| serde_json::json!({"kind": "page", "id": id});
            let layout = serde_json::json!({
                "groups": [
                    {"pick": page(1), "share": 0.5, "tabs": [page(1)]},
                    {"pick": page(refused), "share": 0.5, "tabs": [page(refused), page(2)]},
                ],
                "active": 1,
            });
            // As on a first show of the workspace since the restart.
            let key = view.browser_key().unwrap();
            view.browser.layouts.remove(&key);
            view.browser
                .saved
                .insert(key, serde_json::from_value(layout).unwrap());

            configure(view);
            view.show_activity_panel(ActivityPanel::Walkr, window, cx);
            view.poll_walkr_page(std::time::Instant::now(), window, cx);
            view.apply_page_event(
                Event::Loaded(TabId::WALKR, "https://walkr.example.test/review".into()),
                window,
                cx,
            );
            assert!(view.browser.pages.contains(TabId::WALKR));
            let page_for = view.walkr.page_for.clone();
            assert!(page_for.is_some());

            view.ensure_layout();
            view.poll_browser(window, cx);

            assert!(view.browser.pages.contains(TabId::WALKR));
            assert_eq!(view.walkr.page_for, page_for);
            assert_eq!(view.walkr.connectivity(), Connectivity::Ready);
            let store = cx.global::<Store>();
            assert!(store.get(TabId::WALKR).is_none());
            assert!(store.get(TabId::from(1)).is_some());
            assert!(store.get(TabId::from(2)).is_some());
            let named: Vec<&Pick> = view
                .browser
                .layouts
                .values()
                .flat_map(|l| l.tabs())
                .collect();
            assert!(!named.contains(&&Pick::Page(TabId::WALKR)));
            assert!(named.contains(&&Pick::Page(TabId::from(1))));
            assert!(named.contains(&&Pick::Page(TabId::from(2))));
        });
    });
}
