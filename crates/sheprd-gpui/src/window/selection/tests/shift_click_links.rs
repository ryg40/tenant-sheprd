//! Shift opens a link with a click, as the link modifier does, also in a
//! pane whose program reads the mouse, and still selects with a drag. A
//! Shift drag where the program reads the mouse is in `gestures`.

use super::*;
use gpui::{ClipboardItem, Entity, VisualTestContext};

const URL: &str = "https://example.com/x";

fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Default::default()
    }
}

/// A window whose pane shows `URL` on its first row, and the place of a
/// column on that row.
fn showing_url(
    mouse_reporting: bool,
    cx: &mut TestAppContext,
) -> (
    Entity<SheprdWindow>,
    &mut VisualTestContext,
    impl Fn(f32) -> Point<Pixels>,
) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        let mut frame = surface(&[URL], 24);
        let snapshot = view.live.snapshot.as_ref().unwrap();
        frame.boot_id = snapshot.boot_id.clone();
        frame.projection_revision = snapshot.revision;
        frame.panes[0].mouse_reporting = mouse_reporting;
        view.live.surface = Some(Arc::new(frame));
        view
    });
    cx.update(|window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string("kept".into()));
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let (origin, width) = view.read_with(cx, |view, _| (view.bounds.origin, view.cell_width));
    (view, cx, move |column: f32| {
        origin + point(px(column * width), px(10.))
    })
}

fn shift_click_opens(mouse_reporting: bool, cx: &mut TestAppContext) {
    let (_view, cx, at) = showing_url(mouse_reporting, cx);
    cx.simulate_click(at(1.), shift());
    assert_eq!(cx.opened_url().as_deref(), Some(URL));
    assert_eq!(
        cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())),
        Some("kept".into())
    );
}

fn shift_drag_selects(cx: &mut TestAppContext) {
    let (view, cx, at) = showing_url(false, cx);
    cx.simulate_mouse_down(at(0.), MouseButton::Left, shift());
    assert!(view.read_with(cx, |view, _| view.selection.is_some()));
    cx.simulate_mouse_move(at(20.6), MouseButton::Left, shift());
    cx.simulate_mouse_up(at(20.6), MouseButton::Left, shift());
    assert!(cx.opened_url().is_none());
    assert_eq!(
        cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())),
        Some(URL.into())
    );
}

#[gpui::test]
fn a_shift_click_on_a_link_opens_it(cx: &mut TestAppContext) {
    shift_click_opens(false, cx);
}

#[gpui::test]
fn a_shift_click_on_a_link_opens_it_where_the_program_reads_the_mouse(cx: &mut TestAppContext) {
    shift_click_opens(true, cx);
}

#[gpui::test]
fn a_shift_drag_across_a_link_selects_it(cx: &mut TestAppContext) {
    shift_drag_selects(cx);
}
