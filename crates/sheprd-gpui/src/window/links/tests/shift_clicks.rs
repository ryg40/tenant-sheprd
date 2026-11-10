//! Shift opens a link with a click, so a Shift hover asks the daemon about
//! a web address that wraps, as the link modifier does.

use super::*;

#[gpui::test]
fn a_shift_click_on_a_wrapped_link_opens_it(cx: &mut gpui::TestAppContext) {
    let mut peer = MockPeer::advertising(&["pane.link.resolve", "pane.link.activate"]);
    let (view, cx) = window(&peer, true, cx);
    let shift = Modifiers {
        shift: true,
        ..Default::default()
    };
    let continuation = at(&view, cx, 2, 1);
    view.read_with(cx, |view, _| {
        assert!(view.terminal_link_at(continuation).is_none());
    });

    cx.simulate_mouse_move(continuation, None, shift);
    resolve_after_delay(cx);
    let request = peer.request();
    assert_eq!(request["method"], "pane.link.resolve");
    assert_eq!(request["params"]["viewport_row"], 1);
    answer(
        &mut peer,
        &view,
        cx,
        &request,
        json!({"type": "pane_link_resolved", "regions": [
            {"row": 0, "start_col": 0, "end_col": 19},
            {"row": 1, "start_col": 0, "end_col": 11},
        ]}),
    );
    // Shift alone shows no underline; the link modifier still does.
    view.read_with(cx, |view, _| {
        assert!(view.hovered_daemon_link().is_some());
        assert!(
            view.underlined_link_rows(view.live.surface.as_deref())
                .is_empty()
        );
    });
    cx.simulate_mouse_move(continuation, None, crate::keymap::secondary_key());
    view.read_with(cx, |view, _| {
        let rows = view.underlined_link_rows(view.live.surface.as_deref());
        assert_eq!(rows, vec![(0, 0..20), (1, 0..12)]);
    });

    cx.simulate_click(continuation, shift);
    let request = peer.request();
    assert_eq!(request["method"], "pane.link.activate");
    assert_eq!(request["params"]["viewport_row"], 1);
    answer(
        &mut peer,
        &view,
        cx,
        &request,
        json!({"type": "pane_link_activated", "url": URL, "handled": false}),
    );
    assert_eq!(cx.opened_url().as_deref(), Some(URL));
}

/// A Shift press on a link may start a selection, so it focuses the pane
/// it lands in, as a press elsewhere does; the click still opens the link.
#[gpui::test]
fn a_shift_press_on_a_link_focuses_its_pane(cx: &mut gpui::TestAppContext) {
    let mut peer = MockPeer::advertising(&["pane.focus"]);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        peer.prepare(&mut view, cx);
        view.live.surface = Some(wrapped(2));
        // Another pane has the focus, and so does another window.
        Arc::make_mut(view.live.snapshot.as_mut().unwrap()).focused_pane_id = Some("w1:p2".into());
        view
    });
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
        window.blur(cx);
        // The daemon answers with a frame of the size this window asked for,
        // the address on its first row, so the pane takes input.
        view.update(cx, |view, _| {
            let size = view.options.surface_size;
            let surface = Arc::make_mut(view.live.surface.as_mut().unwrap());
            let rect = SurfaceRect {
                x: 0,
                y: 0,
                width: size.cols,
                height: size.rows,
            };
            let blank = surface.frame.cells[usize::from(surface.frame.width) * 2].clone();
            let mut symbols = URL.chars();
            surface.frame.cells = (0..usize::from(size.cols) * usize::from(size.rows))
                .map(|_| CellData {
                    symbol: symbols
                        .next()
                        .map_or_else(|| blank.symbol.clone(), String::from),
                    ..blank.clone()
                })
                .collect();
            (surface.frame.width, surface.frame.height) = (size.cols, size.rows);
            surface.panes[0].rect = rect;
            surface.panes[0].inner_rect = rect;
        });
    });
    let shift = Modifiers {
        shift: true,
        ..Default::default()
    };
    let on_link = at(&view, cx, 1, 0);

    cx.simulate_mouse_down(on_link, gpui::MouseButton::Left, shift);
    cx.update(|window, cx| assert!(view.read(cx).focus.is_focused(window)));
    let request = peer.request();
    assert_eq!(request["method"], "pane.focus");
    assert_eq!(request["params"]["pane_id"], "w1:p1");

    cx.simulate_mouse_up(on_link, gpui::MouseButton::Left, shift);
    assert!(cx.opened_url().is_some());
}
