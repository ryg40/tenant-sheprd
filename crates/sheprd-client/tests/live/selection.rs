//! The daemon contract a mouse copy relies on: `pane.selection.read` joins
//! the rows the terminal soft-wrapped a long line onto, as Herdr's own copy
//! does, so the line pastes as one line.

use super::{
    plugins::{enter, row_text},
    *,
};
use sheprd_client::{
    protocol::PaneSurfaceFrame,
    scrollback::{ScrollbackResponse, SelectionReadParams, TextPoint, decode_response},
};

#[test]
#[ignore = "requires explicit HERDR_TEST_BINARY; spawns an isolated live daemon"]
fn selection_read_joins_soft_wrapped_rows_live() {
    let daemon = Daemon::start();
    let mut session = Session::open(&daemon);
    let snapshot = session.snapshot.clone().unwrap();
    let boot = snapshot.boot_id.clone();
    let pane = snapshot.focused_pane_id.clone().unwrap();
    session
        .client
        .handle
        .resize(
            &boot,
            ConnectOptions {
                surface_size: ClientSurfaceSize { cols: 50, rows: 20 },
                ..ConnectOptions::default()
            },
        )
        .unwrap();
    let mut width = 0;
    session.until("50-column surface", |event| match event {
        ClientEvent::Surface(surface) if surface.frame.width == 50 => {
            width = surface
                .panes
                .iter()
                .find(|painted| painted.pane_id == pane)
                .map_or(0, |painted| painted.inner_rect.width);
            width > 0
        }
        _ => false,
    });

    // Two full rows and a short third: the shell writes no newline between them.
    let length = usize::from(width) * 2 + 7;
    let line = "W".repeat(length);
    session
        .client
        .handle
        .send_input(
            &boot,
            &pane,
            vec![
                ClientPaneInputEvent::TextCommit(format!(
                    "head -c {length} /dev/zero | tr '\\0' W; echo"
                )),
                enter(),
            ],
        )
        .unwrap();
    let mut found = None;
    session.until("wrapped line", |event| match event {
        ClientEvent::Surface(surface) => {
            found = wrapped(surface, &pane, width).map(|row| (surface.clone(), row));
            found.is_some()
        }
        _ => false,
    });
    let (surface, row) = found.unwrap();
    let painted = surface
        .panes
        .iter()
        .find(|painted| painted.pane_id == pane)
        .unwrap();
    let top = painted.scroll.map_or(0, |scroll| {
        u32::try_from(scroll.max_offset_from_bottom - scroll.offset_from_bottom).unwrap()
    });
    let first = top + u32::from(row - painted.inner_rect.y);
    // From the first row's left edge through its third row's right edge, as
    // a drag to the pane's edge selects it.
    let params = SelectionReadParams {
        pane_id: pane.clone(),
        anchor: TextPoint { row: first, col: 0 },
        cursor: TextPoint {
            row: first + 2,
            col: width - 1,
        },
        content_revision: None,
    };
    let id = session
        .client
        .handle
        .read_selection(&boot, &params)
        .unwrap();
    let mut response = None;
    session.until("selection read", |event| match event {
        ClientEvent::Response {
            request_id,
            response: body,
        } if *request_id == id => {
            response = Some(body.clone());
            true
        }
        _ => false,
    });
    let ScrollbackResponse::PaneSelection(selection) = decode_response(&response.unwrap()).unwrap()
    else {
        panic!("not a selection");
    };
    assert_eq!(selection.text, line, "{:?}", selection.text);
    eprintln!(
        "pane.selection.read joined {length} characters over 3 rows of {width}: {:?}",
        selection.text
    );
}

/// The frame row where the printed line starts: two full rows of `W`, then
/// the rest, then the prompt, so no more output is coming.
fn wrapped(surface: &PaneSurfaceFrame, pane_id: &str, width: u16) -> Option<u16> {
    let painted = surface.panes.iter().find(|pane| pane.pane_id == pane_id)?;
    let rect = painted.inner_rect;
    let full = "W".repeat(usize::from(width));
    let pane_row = |row: u16| -> String {
        row_text(surface, row)
            .chars()
            .skip(usize::from(rect.x))
            .take(usize::from(width))
            .collect()
    };
    (rect.y..rect.y + rect.height.saturating_sub(3)).find(|&row| {
        pane_row(row) == full
            && pane_row(row + 1) == full
            && pane_row(row + 2).trim_end() == "W".repeat(7)
            && pane_row(row + 3).starts_with("LIVE>")
    })
}
