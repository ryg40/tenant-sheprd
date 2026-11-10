use super::*;
use crate::window::MockPeer;
use serde_json::{Value, json};

/// A window whose pane paints `rows` and whose daemon advertises
/// `pane.selection.read` when `reads` is set.
fn window(
    cx: &mut TestAppContext,
    reads: bool,
) -> (
    MockPeer,
    gpui::Entity<SheprdWindow>,
    &mut gpui::VisualTestContext,
) {
    let methods: &[&str] = if reads { &["pane.selection.read"] } else { &[] };
    let peer = MockPeer::advertising(methods);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        peer.prepare(&mut view, cx);
        view.live.supports_selection_read = reads;
        // The pane's grid is exactly the painted frame, so the local text
        // has every cell it covers.
        let mut frame = surface(&["wrapped lo", "ng line"], 10);
        let snapshot = view.live.snapshot.as_ref().unwrap();
        frame.boot_id = snapshot.boot_id.clone();
        frame.projection_revision = snapshot.revision;
        frame.panes[0].pane_id = "w1:p1".into();
        view.live.surface = Some(Arc::new(frame));
        view
    });
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    (peer, view, cx)
}

/// Drags from the first cell through the end of the second row and releases.
fn select_both_rows(view: &gpui::Entity<SheprdWindow>, cx: &mut gpui::VisualTestContext) {
    let (origin, cell) = view.read_with(cx, |view, _| {
        (
            view.bounds.origin,
            (view.cell_width, view.config.terminal.line_height()),
        )
    });
    let at = |column: f32, row: f32| origin + point(px(column * cell.0), px(row * cell.1));
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("before".into())));
    cx.simulate_mouse_down(at(0.2, 0.5), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(at(9.8, 1.5), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(at(9.8, 1.5), MouseButton::Left, Modifiers::default());
}

fn selection_read(peer: &mut MockPeer) -> Value {
    loop {
        let request = peer.request();
        if request["method"] == "pane.selection.read" {
            return request;
        }
        let id = request["id"].as_str().unwrap();
        peer.respond("boot-v1", id, &json!({"id": id, "result": {"type": "ok"}}));
    }
}

/// Delivers `result` for `request` as the connection's reader does, then
/// lets the window copy it.
fn answer(
    peer: &mut MockPeer,
    view: &gpui::Entity<SheprdWindow>,
    cx: &mut gpui::VisualTestContext,
    request: &Value,
    result: Value,
) {
    let id = request["id"].as_str().unwrap();
    let mut body = json!({"id": id});
    body.as_object_mut()
        .unwrap()
        .extend(result.as_object().unwrap().clone());
    let event = peer.respond("boot-v1", id, &body);
    let inbox = view.read_with(cx, |view, _| {
        view.endpoints[0].connection.scrollback.clone()
    });
    assert!(inbox.lock().unwrap().apply(event).is_none());
    view.update(cx, |view, cx| view.follow_selection(cx));
}

fn clipboard(cx: &mut gpui::VisualTestContext) -> Option<String> {
    cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
}

/// A selection the screen shows copies the daemon's text for its range,
/// which joins the soft-wrapped rows the painted cells split.
#[gpui::test]
fn an_on_screen_pane_selection_copies_the_daemon_text(cx: &mut TestAppContext) {
    let (mut peer, view, cx) = window(cx, true);
    select_both_rows(&view, cx);
    let read = selection_read(&mut peer);
    assert_eq!(
        read["params"],
        json!({
            "pane_id": "w1:p1",
            "anchor": {"row": 0, "col": 0},
            "cursor": {"row": 1, "col": 9},
        })
    );
    // Nothing reaches the clipboard before the daemon answers.
    assert_eq!(clipboard(cx).as_deref(), Some("before"));
    answer(
        &mut peer,
        &view,
        cx,
        &read,
        json!({"result": {"type": "pane_selection", "pane_id": "w1:p1",
            "text": "wrapped long line"}}),
    );
    assert_eq!(clipboard(cx).as_deref(), Some("wrapped long line"));
}

/// A failed read reports the error and leaves the clipboard alone: the
/// painted text, with its row breaks, is never copied in its place.
#[gpui::test]
fn a_failed_read_copies_nothing(cx: &mut TestAppContext) {
    let (mut peer, view, cx) = window(cx, true);
    select_both_rows(&view, cx);
    let read = selection_read(&mut peer);
    answer(
        &mut peer,
        &view,
        cx,
        &read,
        json!({"error": {"code": "stale_content", "message": "changed"}}),
    );
    assert_eq!(clipboard(cx).as_deref(), Some("before"));
    view.read_with(cx, |view, _| {
        assert!(view.local_error.is_some());
        assert!(view.selection_follow.read.is_none());
    });
}

/// A daemon without `pane.selection.read` gets no request: the painted
/// cells give the text at once.
#[gpui::test]
fn without_the_method_the_painted_text_is_copied(cx: &mut TestAppContext) {
    let (_peer, view, cx) = window(cx, false);
    select_both_rows(&view, cx);
    assert_eq!(clipboard(cx).as_deref(), Some("wrapped lo\nng line"));
    view.read_with(cx, |view, _| {
        assert!(view.selection_follow.read.is_none());
        assert!(view.local_error.is_none());
    });
}

/// The newest copy wins: a read for an older selection that answers after
/// a newer selection copied its painted text leaves that text alone.
#[gpui::test]
fn a_late_read_does_not_replace_a_newer_local_copy(cx: &mut TestAppContext) {
    let (mut peer, view, cx) = window(cx, true);
    select_both_rows(&view, cx);
    let read = selection_read(&mut peer);
    // The next selection copies the painted cells at once, as a popup's does.
    view.update(cx, |view, _| view.live.supports_selection_read = false);
    select_both_rows(&view, cx);
    assert_eq!(clipboard(cx).as_deref(), Some("wrapped lo\nng line"));
    answer(
        &mut peer,
        &view,
        cx,
        &read,
        json!({"result": {"type": "pane_selection", "pane_id": "w1:p1",
            "text": "wrapped long line"}}),
    );
    assert_eq!(clipboard(cx).as_deref(), Some("wrapped lo\nng line"));
}
