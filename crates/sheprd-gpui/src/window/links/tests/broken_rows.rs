//! A web address that the program in the pane broke over rows itself: the
//! daemon reads one row of it, so the local reading is underlined and opens.

use super::*;

const WHOLE: &str = "https://e.com/a/0123456789/x1";
/// What the daemon reads at a cell of the first row: the row alone.
const CUT: &str = "https://e.com/a/0123";

/// The 20-column pane with `WHOLE` broken at its edge, the rest two columns
/// in on the second row.
fn broken() -> Arc<PaneSurfaceFrame> {
    let mut surface = wrapped(2);
    let text = format!("{CUT}  456789/x1 ok");
    let mut symbols = text.chars();
    for cell in &mut Arc::make_mut(&mut surface).frame.cells {
        cell.symbol = symbols.next().unwrap_or(' ').to_string();
    }
    surface
}

#[gpui::test]
fn an_address_the_program_broke_opens_whole_over_the_daemons_reading(
    cx: &mut gpui::TestAppContext,
) {
    let mut peer = MockPeer::advertising(&["pane.link.resolve", "pane.link.activate"]);
    let (view, cx) = window(&peer, true, cx);
    view.update(cx, |view, _| view.live.surface = Some(broken()));
    let shift = Modifiers {
        shift: true,
        ..Default::default()
    };
    let first_row = at(&view, cx, 5, 0);
    let last_row = at(&view, cx, 4, 1);

    // The daemon resolves the first row alone; every row is underlined.
    cx.simulate_mouse_move(first_row, None, crate::keymap::secondary_key());
    resolve_after_delay(cx);
    let request = peer.request();
    assert_eq!(request["method"], "pane.link.resolve");
    answer(
        &mut peer,
        &view,
        cx,
        &request,
        json!({"type": "pane_link_resolved", "regions": [
            {"row": 0, "start_col": 0, "end_col": 19},
        ]}),
    );
    view.read_with(cx, |view, _| {
        let rows = view.underlined_link_rows(view.live.surface.as_deref());
        assert_eq!(rows, vec![(0, 0..20), (1, 2..11)]);
    });

    // The daemon answers a click on the first row with the cut address.
    cx.simulate_click(first_row, shift);
    let request = peer.request();
    assert_eq!(request["method"], "pane.link.activate");
    answer(
        &mut peer,
        &view,
        cx,
        &request,
        json!({"type": "pane_link_activated", "url": CUT, "handled": false}),
    );
    assert_eq!(cx.opened_url().as_deref(), Some(WHOLE));

    // On the last row the daemon reads no address at all.
    cx.simulate_mouse_move(last_row, None, shift);
    cx.simulate_click(last_row, shift);
    let request = peer.request();
    assert_eq!(request["method"], "pane.link.activate");
    answer(
        &mut peer,
        &view,
        cx,
        &request,
        json!({"type": "pane_link_activated", "url": null, "handled": false}),
    );
    assert_eq!(cx.opened_url().as_deref(), Some(WHOLE));
}
