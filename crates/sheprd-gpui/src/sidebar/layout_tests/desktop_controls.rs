//! The desktop layout's icon controls: the Spaces heading's `+` and `...`,
//! which stand in for the `new` and `menu` links, the Agents heading's sort
//! and group buttons, and the footer's rows.
use super::*;
use crate::{config::LayoutMode, window::MockPeer};
use gpui::{
    Element, IntoElement,
    accesskit::{Node, Role},
};

fn draw(cx: &mut gpui::VisualTestContext) {
    cx.update(|window, cx| full_draw(window, cx).clear(cx));
}

/// Presses `key` on whatever holds the keyboard.
fn press(key: &str, cx: &mut gpui::VisualTestContext) {
    let keystroke = crate::keymap::parse_keystroke(key).unwrap();
    cx.simulate_event(gpui::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(gpui::KeyUpEvent { keystroke });
}

/// `+` creates a workspace and `...` opens the menu from the end of the
/// heading in the desktop layout; under the list, the links still do in
/// the others.
#[gpui::test]
fn new_and_menu_act_from_the_heading_or_the_links(cx: &mut gpui::TestAppContext) {
    for mode in [LayoutMode::Desktop, LayoutMode::default()] {
        let mut peer = MockPeer::advertising(&["workspace.create"]);
        let (view, cx) = cx.add_window_view(fixture_window);
        cx.simulate_resize(size(px(800.), px(600.)));
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                peer.prepare(view, cx);
                view.config.layout.mode = mode;
            })
        });
        draw(cx);
        let heading = cx.debug_bounds("header-spaces").unwrap();
        let new = cx.debug_bounds("new-workspace").unwrap();
        let menu = cx.debug_bounds("sidebar-menu").unwrap();
        let in_heading = |control: Bounds<Pixels>| {
            control.top() >= heading.top() && control.bottom() <= heading.bottom()
        };
        let desktop = mode == LayoutMode::Desktop;
        assert_eq!(in_heading(new), desktop, "{mode}: {new:?} {heading:?}");
        assert_eq!(in_heading(menu), desktop, "{mode}: {menu:?} {heading:?}");
        if desktop {
            // At the heading's end, inside the sidebar.
            assert!(new.right() <= menu.left(), "{new:?} {menu:?}");
            assert!(menu.right() <= heading.right(), "{menu:?} {heading:?}");
            assert!(new.left() > heading.center().x, "{new:?} {heading:?}");
        }
        cx.simulate_click(new.center(), Modifiers::default());
        assert_eq!(peer.request()["method"], "workspace.create", "{mode}");
        cx.simulate_click(menu.center(), Modifiers::default());
        view.read_with(cx, |view, _| {
            assert_eq!(view.menu.page, Some(crate::menu::Page::Menu), "{mode}");
        });
    }
}

/// The heading's buttons are named for screen readers, and the keyboard
/// reaches them among the sidebar's stops.
#[gpui::test]
fn the_heading_buttons_are_named_stops(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.config.layout.mode = LayoutMode::Desktop;
        view
    });
    cx.simulate_resize(size(px(800.), px(600.)));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let buttons = [
                (view.new_workspace_button(cx), "New workspace"),
                (view.menu_button(cx), "Menu"),
            ];
            for (button, name) in buttons {
                let element = button.into_element();
                let mut node = Node::new(element.a11y_role().unwrap());
                element.write_a11y_info(&mut node);
                assert_eq!(node.role(), Role::Button);
                assert_eq!(node.label(), Some(name));
            }
        })
    });
    // Focus events reach listeners only in the active window.
    cx.update(|window, _| window.activate_window());
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.chrome_region = Some(crate::window::chrome_focus::Region::Sidebar);
        })
    });
    draw(cx);
    // The heading leads the sidebar, so Tab reaches its controls before
    // any row; Enter on `...` opens the menu.
    for _ in 0..3 {
        cx.update(|window, cx| window.focus_next(cx));
        draw(cx);
        press("enter", cx);
        draw(cx);
        if view.read_with(cx, |view, _| view.menu.page.is_some()) {
            break;
        }
    }
    view.read_with(cx, |view, _| {
        assert_eq!(view.menu.page, Some(crate::menu::Page::Menu));
        assert!(view.pending_navigation.is_none());
    });
}

/// The sort and the group filter are icon buttons that act on a click, and
/// a picked group's name gives way before the sort leaves a narrow header.
#[gpui::test]
fn the_agents_heading_buttons_sort_and_filter(cx: &mut gpui::TestAppContext) {
    let long = "a-coordinator-group-with-a-very-long-name-indeed";
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = agent_groups::grouped(window, cx);
        let snapshot = Arc::make_mut(view.live.snapshot.as_mut().unwrap());
        snapshot.agents[2].name = Some(format!("{long}-coord"));
        view.config.layout.mode = LayoutMode::Desktop;
        view.sidebar_width = Some(160.);
        view
    });
    cx.simulate_resize(size(px(800.), px(600.)));
    draw(cx);
    let sort = view.read_with(cx, |view, _| view.agent_sort);
    let button = cx.debug_bounds("agents-sort").unwrap();
    cx.simulate_click(button.center(), Modifiers::default());
    draw(cx);
    view.read_with(cx, |view, _| assert_eq!(view.agent_sort, sort.toggled()));
    // No group yet: the filter is the bare icon.
    assert!(cx.debug_bounds("agents-group-name").is_none());
    let group = cx.debug_bounds("agents-group").unwrap();
    cx.simulate_click(group.center(), Modifiers::default());
    draw(cx);
    view.read_with(cx, |view, _| {
        assert_eq!(view.agent_group.as_deref(), Some(long));
    });
    let header = cx.debug_bounds("header-agents").unwrap();
    let group = cx.debug_bounds("agents-group").unwrap();
    let sort = cx.debug_bounds("agents-sort").unwrap();
    let name = cx.debug_bounds("agents-group-name").unwrap();
    assert!(sort.right() <= header.right(), "{sort:?} past {header:?}");
    assert!(group.right() <= sort.left(), "{group:?} over {sort:?}");
    assert!(name.right() <= group.right(), "{name:?} out of {group:?}");
}

/// The footer's rows stay inside a narrow sidebar both ways, mirror right
/// to left, and drop Settings beside the activity bar, which has its own.
#[gpui::test]
fn the_footer_rows_fit_a_narrow_sidebar_both_ways(cx: &mut gpui::TestAppContext) {
    for rtl in [false, true] {
        if rtl {
            crate::i18n::show(&sheprd_i18n::Pseudo::Bidi.locale());
        }
        for bar in [false, true] {
            let (view, cx) = cx.add_window_view(|window, cx| {
                let mut view = fixture_window(window, cx);
                view.config.layout.mode = LayoutMode::Desktop;
                view.config.sidebar_style.activity_bar = bar;
                let mut remote = crate::endpoint::Endpoint::new(
                    "ssh:fixture".into(),
                    "A very long remote device label that must fit".into(),
                    ConnectTarget::Socket("/unused-desktop-footer-test.sock".into()),
                    true,
                );
                remote.live.snapshot = view.live.snapshot.clone();
                view.endpoints.push(remote);
                view.sidebar_width = Some(140.);
                view
            });
            cx.simulate_resize(size(px(800.), px(600.)));
            draw(cx);
            let case = format!("rtl {rtl}, bar {bar}");
            let sidebar = cx.debug_bounds("sidebar").unwrap();
            let footer = cx.debug_bounds("device-footer").unwrap();
            let picker = cx.debug_bounds("device-picker").unwrap();
            let icon = cx.debug_bounds("device-footer-icon").unwrap();
            let label = cx.debug_bounds("device-footer-label").unwrap();
            assert_eq!(footer.bottom(), sidebar.bottom(), "{case}");
            assert!(
                cx.debug_bounds("agents-scroll").unwrap().bottom() <= footer.top(),
                "{case}"
            );
            let height = view.read_with(cx, |view, _| view.device_footer_height());
            assert_eq!(footer.size.height, px(height), "{case}");
            let settings = cx.debug_bounds("device-settings");
            assert_eq!(settings.is_none(), bar, "{case}");
            for row in [Some(picker), settings].into_iter().flatten() {
                assert!(row.left() >= footer.left(), "{case}: {row:?} {footer:?}");
                assert!(row.right() <= footer.right(), "{case}: {row:?} {footer:?}");
                assert!(
                    row.bottom() <= footer.bottom(),
                    "{case}: {row:?} {footer:?}"
                );
            }
            if rtl {
                assert!(icon.left() >= label.right(), "{case}: {icon:?} {label:?}");
            } else {
                assert!(icon.right() <= label.left(), "{case}: {icon:?} {label:?}");
            }
            assert!(label.size.width > px(0.), "{case}: {label:?}");
        }
    }
    crate::i18n::show(&sheprd_i18n::Locale::FALLBACK);
}

/// A large `[sidebar] size` makes the footer's rows taller rather than
/// running their labels out of them, and the footer drawn is as tall as
/// the list above it leaves room for.
#[gpui::test]
fn the_footer_rows_grow_with_a_large_sidebar_size(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.config.layout.mode = LayoutMode::Desktop;
        view.config.sidebar_style.device_footer = crate::config::DeviceFooter::Always;
        view.config.sidebar_style.activity_bar = false;
        view.config.sidebar.size = 24.;
        view.config.sidebar_font_keys.size = true;
        view
    });
    cx.simulate_resize(size(px(800.), px(600.)));
    draw(cx);
    let footer = cx.debug_bounds("device-footer").unwrap();
    let height = view.read_with(cx, |view, _| view.device_footer_height());
    assert_eq!(footer.size.height, px(height));
    let label = cx.debug_bounds("device-footer-label").unwrap();
    for id in ["device-picker", "device-settings"] {
        let row = cx.debug_bounds(id).unwrap();
        assert!(
            label.size.height <= row.size.height,
            "{id}: {label:?} {row:?}"
        );
        assert!(row.bottom() <= footer.bottom(), "{id}: {row:?} {footer:?}");
    }
}
