//! Web addresses that an agent broke over rows itself, in the shapes Claude
//! Code 2.1.322 draws in a Herdr pane: no hyperlink, no terminal wrap, the
//! rest of the line after an indent on the next row.

use super::*;

/// A frame whose rows each hold one line, padded with blanks to `width`.
fn lines(rows: &[String], width: u16) -> FrameData {
    let mut frame = frame("", width, rows.len() as u16);
    for (row, text) in rows.iter().enumerate() {
        let start = row * usize::from(width);
        for (cell, symbol) in frame.cells[start..].iter_mut().zip(text.chars()) {
            cell.symbol = symbol.to_string();
        }
    }
    frame
}

/// `line` as the agent draws it: broken wherever it reaches column `limit`,
/// with each next row after `indent` blanks.
fn broken(line: &str, limit: usize, indent: usize) -> Vec<String> {
    let mut rest: Vec<char> = line.chars().collect();
    let mut rows = Vec::new();
    let mut room = limit;
    while rest.len() > room {
        let tail = rest.split_off(room);
        rows.push(rest.into_iter().collect::<String>());
        rest = tail;
        room = limit - indent;
    }
    rows.push(rest.into_iter().collect());
    rows.iter()
        .enumerate()
        .map(|(row, text)| format!("{}{text}", " ".repeat(if row == 0 { 0 } else { indent })))
        .collect()
}

fn read(frame: &FrameData, column: u16, row: u16) -> Option<RowLink> {
    frame_link(frame, column, row, 0..frame.width, 0..frame.height)
}

const LONG: &str = concat!(
    "https://claude-wrap-check.example.test/threads/",
    "f549cdd7a0cd4f789f99654f1cbe7c24-f549cdd7a0cd4f789f99654f1cbe7c24-",
    "f549cdd7a0cd4f789f99654f1cbe7c24-f549cdd7a0cd4f789f99654f1cbe7c24-",
    "f549cdd7a0cd4f789f99654f1cbe7c24-f549cdd7a0cd4f789f99654f1cbe7c24/end?x=1",
);

#[test]
fn an_address_broken_in_prose_opens_whole_from_each_row() {
    // Prose fills its rows to the pane's edge and goes on under the text,
    // two columns in.
    let width = 120;
    let rows = broken(
        &format!("● Test long URL: {LONG} answers 200."),
        usize::from(width),
        2,
    );
    let frame = lines(&rows, width);
    let whole = read(&frame, 20, 0).unwrap();
    assert_eq!(whole.target, RowTarget::Web(LONG.into()));
    assert_eq!(
        whole.rows().collect::<Vec<_>>(),
        vec![(0, 17..120), (1, 2..120), (2, 2..33)]
    );
    for (column, row) in [
        (17, 0),
        (119, 0),
        (2, 1),
        (60, 1),
        (119, 1),
        (2, 2),
        (32, 2),
    ] {
        assert_eq!(
            read(&frame, column, row).as_ref(),
            Some(&whole),
            "({column}, {row})"
        );
    }
    // The words around it are no part of it.
    assert!(
        [(8, 0), (34, 2), (0, 1)]
            .into_iter()
            .all(|(column, row)| read(&frame, column, row).as_ref() != Some(&whole))
    );
}

#[test]
fn an_address_broken_in_command_output_opens_whole() {
    // Command output starts five columns in and ends five columns before
    // the pane's edge, here inside the address of a line of JSON.
    let url = concat!(
        "https://app.example.test/threads/f549cdd7-a0cd-4f78-9f99-654f1cbe7c24/",
        "mcp%3A32667a31-d107-4dfa-af50-d90059380506",
    );
    let line = format!(
        concat!(
            "     {}e\":\"scout: probe-task.md\",\"link\":\"{}\",\"createdBy\":\"agent\",",
            "\"creationSource\":\"mcp\",\"status\":\"completed\",",
            "\"latestRunId\":\"run:thread:mcp%3A32667a31-d107-4dfa-af50-d90059380506\"}}]",
        ),
        "x".repeat(65),
        url
    );
    let rows = broken(&line, 186, 5);
    let frame = lines(&rows, 191);
    let whole = read(&frame, 150, 0).unwrap();
    assert_eq!(whole.target, RowTarget::Web(url.into()));
    assert_eq!(
        whole.rows().collect::<Vec<_>>(),
        vec![(0, 104..186), (1, 5..35)]
    );
    for (column, row) in [(104, 0), (185, 0), (5, 1), (34, 1)] {
        assert_eq!(
            read(&frame, column, row).as_ref(),
            Some(&whole),
            "({column}, {row})"
        );
    }
}

#[test]
fn an_address_that_ends_its_row_before_a_new_word_stays_alone() {
    // The agent broke this line at a blank: the address ends at the pane's
    // edge, and the row below starts a new word that would have fit beside
    // it in one row.
    let item = "  - [2] Done: the page that the owner asked for is at https://t3code.example.test/";
    let width = item.chars().count() as u16;
    let frame = lines(&[item.into(), "    answers\u{a0}200.".into()], width);
    let alone = read(&frame, width - 5, 0).unwrap();
    assert_eq!(
        alone.target,
        RowTarget::Web("https://t3code.example.test/".into())
    );
    assert_eq!(
        alone.rows().collect::<Vec<_>>(),
        vec![(0, width - 28..width)]
    );
    assert!(read(&frame, 6, 1).is_none());
}

/// An address of `length` characters whose every stretch has digits.
fn address(length: usize) -> String {
    "https://example.test/"
        .chars()
        .chain("0123456789abcdef".chars().cycle())
        .take(length)
        .collect()
}

#[test]
fn an_address_that_nearly_fills_its_row_stays_apart_from_the_next_word() {
    // The agent ended the line after the address: the row below starts a
    // new sentence or a list item under the same indent. The address ends
    // at each limit a broken row ends at, and beside them.
    let width = 100;
    for gap in [0, 1, 2, 3, 5] {
        let url = address(usize::from(width) - 2 - gap);
        for below in ["  Let me know if that works.", "  - next item"] {
            let frame = lines(&[format!("  {url}"), below.into()], width);
            let alone = read(&frame, 40, 0).unwrap();
            assert_eq!(
                alone.target,
                RowTarget::Web(url.clone()),
                "{gap} before {below:?}"
            );
            assert_eq!(
                alone.rows().collect::<Vec<_>>(),
                vec![(0, 2..width - gap as u16)]
            );
            assert_ne!(read(&frame, 2, 1), Some(alone));
        }
    }
}

#[test]
fn addresses_on_consecutive_rows_open_alone() {
    // An address that ends at the pane's edge is the daemon's to read, as
    // one the terminal may have wrapped, so the rows end before the edge.
    let width = 100;
    for gap in [1, 3, 5] {
        let length = usize::from(width) - 2 - gap;
        let first = address(length);
        let second = format!("{}/second", address(length - 7));
        let frame = lines(
            &[format!("  {first}"), format!("  {second}"), String::new()],
            width,
        );
        for (row, url) in [(0, &first), (1, &second)] {
            let link = read(&frame, 40, row).unwrap();
            assert_eq!(link.target, RowTarget::Web(url.clone()), "{gap}, {row}");
            assert_eq!(
                link.rows().collect::<Vec<_>>(),
                vec![(row, 2..width - gap as u16)]
            );
        }
    }
}

#[test]
fn an_address_is_read_across_sixteen_rows_and_no_more() {
    // Rows of 40 columns: the first holds 36 characters of the address, each
    // next one 38, and the last one 10.
    let width = 40;
    let over = |rows: usize| {
        let url = address(36 + 38 * (rows - 2) + 10);
        let mut text = broken(&format!("see {url}"), usize::from(width), 2);
        text.push(String::new());
        (lines(&text, width), url)
    };
    let (frame, url) = over(16);
    for row in [0, 8, 15] {
        assert_eq!(
            read(&frame, 6, row).map(|link| link.target),
            Some(RowTarget::Web(url.clone())),
            "row {row}"
        );
    }
    // One row more is not opened cut: no row of it reads as an address.
    let (frame, _) = over(17);
    assert!((0..17).all(|row| !matches!(
        read(&frame, 6, row).map(|link| link.target),
        Some(RowTarget::Web(_))
    )));
}
