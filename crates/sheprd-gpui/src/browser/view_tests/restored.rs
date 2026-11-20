use super::*;
use crate::{browser::engine::inert, sidebar::ActivityPanel};

/// Makes `view`'s workspace as after a restart: a saved layout whose one
/// group shows a tab of `https://shown.test/app` before another of
/// `https://hidden.test/app`, and no page made in this run.
fn restore_two_tabs(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        let scope = scope(&view.read(cx).endpoints[0]);
        let mut open = |address| {
            Store::update(cx, |store| {
                store.open(scope.clone(), "w0", Some(url(address)), None)
            })
            .unwrap()
        };
        let shown = serde_json::to_value(open("https://shown.test/app")).unwrap();
        let hidden = serde_json::to_value(open("https://hidden.test/app")).unwrap();
        let saved = serde_json::json!({
            "groups": [{
                "pick": {"kind": "page", "id": shown},
                "share": 1.0,
                "tabs": [{"kind": "page", "id": shown}, {"kind": "page", "id": hidden}],
            }],
            "active": 0,
        });
        inert::opened();
        view.update(cx, |view, _| {
            // As on a first show of the workspace since the restart.
            let key = view.browser_key().unwrap();
            view.browser.layouts.remove(&key);
            view.browser
                .saved
                .insert(key, serde_json::from_value(saved).unwrap());
        });
    });
}

fn poll(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| view.update(cx, |view, cx| view.poll_browser(window, cx)));
    draw(cx);
}

/// After a restart a saved layout shows a tab that nobody showed in this
/// run. The window makes that tab's page, and none for a tab the layout
/// does not show. The engine is the tests' inert one.
#[gpui::test]
fn a_restored_tab_a_group_shows_gets_its_page(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    restore_two_tabs(&view, cx);
    draw(cx);
    poll(&view, cx);
    assert_eq!(inert::opened(), ["https://shown.test/app"]);
    assert!(cx.debug_bounds("browser-placeholder").is_none());
}

/// A section that takes the window's body hides the groups. The tab a
/// restored group shows behind it gets no page, and its site no request,
/// until the groups are drawn again.
#[gpui::test]
fn a_restored_tab_behind_a_section_waits_until_it_is_drawn(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    restore_two_tabs(&view, cx);
    view.update(cx, |view, _| view.config.sidebar_style.activity_bar = true);
    let show = |panel, cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| view.show_activity_panel(panel, window, cx));
        });
        draw(cx);
    };
    show(ActivityPanel::Overview, cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.activity_section()),
        Some(ActivityPanel::Overview)
    );
    for _ in 0..3 {
        poll(&view, cx);
    }
    assert!(inert::opened().is_empty());

    show(ActivityPanel::Workspaces, cx);
    poll(&view, cx);
    assert_eq!(inert::opened(), ["https://shown.test/app"]);
}
