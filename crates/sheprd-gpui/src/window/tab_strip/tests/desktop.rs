use super::*;
use crate::config::{LayoutMode, TabLayoutMode};

/// The strip of `layouts::strip` in the desktop layout, in a window `width`
/// wide. The file names no tab layout, so its tabs are pills.
fn desktop(cx: &mut TestAppContext, width: f32) -> (Entity<SheprdWindow>, &mut VisualTestContext) {
    let (view, cx) = layouts::strip(cx, TabLayoutMode::Classic);
    view.update(cx, |view, cx| {
        view.config.layout.mode = LayoutMode::Desktop;
        cx.notify();
    });
    cx.simulate_resize(size(px(width), px(600.)));
    for _ in 0..2 {
        cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
        view.update(cx, |view, _| view.settle_tabs());
    }
    (view, cx)
}

/// Tabs in the desktop face draw as wide as the strip measures them, so no
/// title runs under its close button, and the buttons after them stay apart
/// and in the window. Headless text has no real glyph widths: this proves
/// the strip measures in the face it draws, not how that face looks.
#[gpui::test]
fn a_desktop_strip_fits_a_narrow_window(cx: &mut TestAppContext) {
    let width = 480.;
    let (view, cx) = desktop(cx, width);
    let measured: Vec<(String, f32)> = cx.update(|window, cx| {
        let view = view.read(cx);
        assert_eq!(view.config.tab_layout_in_use(), TabLayoutMode::Pills);
        assert_ne!(*view.config.tabs_font(), view.config.tabs);
        let indicators = Indicators::new(
            None,
            crate::config::StatusIcons::default(),
            false,
            false,
            &view.theme,
        );
        let snapshot = view.live.snapshot.as_ref().unwrap();
        snapshot
            .tabs
            .iter()
            .filter(|tab| tab.workspace_id == "w1")
            .map(|tab| {
                let shape = TabShape {
                    lead: herdr_mark(snapshot, tab).lead(),
                    zoomed: tab.zoomed,
                    muted: false,
                    selected: tab.focused,
                };
                let label = SharedString::from(tab.label.clone());
                let width = view.tab_width(&label, shape, indicators, window);
                (format!("tab-{}", tab.tab_id), width)
            })
            .collect()
    });
    assert!(measured.len() >= 3, "{measured:?}");
    let bounds = |cx: &mut VisualTestContext, selector: String| {
        let selector: &'static str = Box::leak(selector.into_boxed_str());
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("no {selector}"))
    };
    for (selector, measured) in measured {
        let tab = bounds(cx, selector.clone());
        assert!(
            (tab.size.width - px(measured)).abs() <= px(0.5),
            "{selector} drew {tab:?}, measured {measured}"
        );
    }
    let tab = bounds(cx, "tab-w1:t1".into());
    let close = bounds(cx, "close-tab-w1:t1".into());
    assert!(close.left() >= tab.left() && close.right() <= tab.right());
    // "+", split, and the group's actions, in order, apart, and in the
    // window, each within the strip's height.
    let strip = bounds(cx, "tab-scroller".into());
    let buttons = ["new-tab", "split-editor", "tab-actions"].map(|id| bounds(cx, id.into()));
    for pair in buttons.windows(2) {
        assert!(pair[0].right() <= pair[1].left(), "{pair:?}");
    }
    for button in buttons {
        assert!(button.right() <= px(width), "{button:?}");
        assert!(button.top() >= strip.top() && button.bottom() <= strip.bottom());
    }
}

/// The desktop layout draws pills only while the user has not chosen a tab
/// layout, so View > Tab Layout > Classic still takes, and is saved.
#[gpui::test]
fn choosing_a_tab_layout_in_desktop_takes_and_saves_it(cx: &mut TestAppContext) {
    let (view, cx) = desktop(cx, 1200.);
    let saved = Arc::new(std::sync::Mutex::new(Vec::new()));
    view.update(cx, |view, cx| {
        let saved = saved.clone();
        view.set_tab_layout_with(
            TabLayoutMode::Classic,
            move |mode| {
                saved.lock().unwrap().push(mode);
                Ok(())
            },
            cx,
        );
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.config.tab_layout_in_use(), TabLayoutMode::Classic);
    });
    assert_eq!(*saved.lock().unwrap(), [TabLayoutMode::Classic]);
}

/// The desktop layout's toggle, an icon button in the glyph's slot, still
/// folds the sidebar away and brings it back.
#[gpui::test]
fn the_desktop_sidebar_toggle_folds_the_sidebar(cx: &mut TestAppContext) {
    let (view, cx) = desktop(cx, 1200.);
    for visible in [false, true] {
        let toggle = cx.debug_bounds("toggle-sidebar").unwrap();
        cx.simulate_click(toggle.center(), Modifiers::default());
        cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
        assert_eq!(view.read_with(cx, |view, _| view.sidebar_visible), visible);
    }
}
