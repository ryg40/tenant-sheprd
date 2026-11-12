//! A web address that the program in the pane broke over rows itself, as an
//! agent does with a line longer than its view: the address stops at the
//! view's right limit and goes on in the next row after that row's indent.
//! The terminal sets no wrap flag for such a break, so the daemon reads only
//! the first row, and this client reads the rows as one address.
//!
//! A program that wraps at blanks breaks a word only when the word is longer
//! than a whole row. So two rows are one word only where all of these hold:
//! - the row above ends at a limit a program was seen to break at
//!   ([`LIMITS`]),
//! - the row below is indented,
//! - the word would not fit in the row below even from its indent, which is
//!   the same as: the indent and the first word below reach past the column
//!   where the word that ends the row above starts,
//! - the first word below reads as the rest of an address ([`tail`]) and
//!   starts no address of its own.
//!
//! A short address that ends its row before a new word, as in
//! `see https://example.com/` over `    for more`, stays alone, and so does
//! one that nearly fills its row before `  Let me know` or `  - next item`.
//!
//! The rows of a broken word and of a line ended at a blank can be the same,
//! so two shapes are read wrong. An address that ends at a limit before an
//! indented word with a digit or one of `/ = % &`, as in `2. Next` or
//! `src/main.rs`, reads as one address with that word. An address broken
//! before a rest of plain letters, as in `getting-sta` over `rted`, is not
//! joined, and its first row reads as the daemon reads it.

use super::{
    MAX_ROW_BYTES, PLAIN_SCHEMES, RowLink, RowTarget, continuations, plain_address, plain_url,
    shown,
    wrap::{Area, MAX_WRAPPED_ROWS, blank},
};
use sheprd_client::protocol::{CellData, FrameData};
use std::ops::Range;

/// The columns a program was seen to leave blank after a row that it broke:
/// an agent's prose ends at the pane's edge, its echo of a command one
/// column before it, and its view of command output five columns before it.
/// A row that ends anywhere else ended its line.
const LIMITS: [usize; 3] = [0, 1, 5];

/// Whether `word`, the first of an indented row, reads as the rest of an
/// address broken in the row above. It has a letter or a digit, and a digit
/// or one of `/ = % &`: identifiers, path steps, and query pairs carry
/// those, and a word of prose seldom does. The marks that prose and lists
/// write are left out: `- . : , ? # + ~ @ _` end a sentence, start a list
/// item or a comment, or stand in a name, so a rest of plain letters with
/// them is not joined. A word that starts with a scheme is an address of
/// its own.
fn tail(word: &str) -> bool {
    word.contains(char::is_alphanumeric)
        && word.contains(|c: char| c.is_ascii_digit() || matches!(c, '/' | '=' | '%' | '&'))
        && !PLAIN_SCHEMES.iter().any(|scheme| word.starts_with(scheme))
}

/// One row of the area, and where its text lies.
struct Row<'a> {
    cells: &'a [CellData],
    /// Whether each cell only continues a wide glyph.
    continuing: Vec<bool>,
    /// The blank cells that start the row.
    indent: usize,
    /// The cell after the last one with text; the indent for an empty row.
    end: usize,
}

impl<'a> Row<'a> {
    fn new(cells: &'a [CellData]) -> Self {
        let continuing = continuations(cells);
        let text = |index: &usize| continuing[*index] || !blank(&cells[*index]);
        let indent = (0..cells.len()).find(text).unwrap_or(cells.len());
        let end = (0..cells.len())
            .rev()
            .find(text)
            .map_or(indent, |last| last + 1);
        Self {
            cells,
            continuing,
            indent,
            end,
        }
    }

    fn blank(&self, index: usize) -> bool {
        !self.continuing[index] && blank(&self.cells[index])
    }

    fn empty(&self) -> bool {
        self.indent >= self.end
    }

    /// Where the word that ends the row starts.
    fn last_word(&self) -> usize {
        (self.indent..self.end)
            .rev()
            .find(|&index| self.blank(index))
            .map_or(self.indent, |at| at + 1)
    }

    /// The cells of the word that starts the row.
    fn first_word(&self) -> usize {
        (self.indent..self.end)
            .find(|&index| self.blank(index))
            .unwrap_or(self.end)
            - self.indent
    }

    /// The text of the word that starts the row.
    fn first_text(&self) -> String {
        (self.indent..self.indent + self.first_word())
            .filter(|&index| !self.continuing[index])
            .map(|index| shown(&self.cells[index]))
            .collect()
    }

    /// Whether the row's text reaches a limit a program breaks its rows at.
    fn full(&self) -> bool {
        !self.empty() && LIMITS.contains(&(self.cells.len() - self.end))
    }

    /// Whether the word that ends this row and the word that starts `below`
    /// are too long to be one word in the row `below`, so a program that
    /// wraps at blanks had to break it.
    fn too_long_with(&self, below: &Self) -> bool {
        below.indent + (self.end - self.last_word()) + below.first_word() > self.end
    }

    /// Whether this is an indented row of text, as a program writes the
    /// rest of a line it broke. A row without an indent is the terminal's
    /// wrap, which the daemon reads from the wrap flags.
    fn indented(&self) -> bool {
        self.indent > 0 && !self.empty()
    }

    /// Whether the word that ends this row goes on in `below`.
    fn joins(&self, below: &Self) -> bool {
        self.full() && below.indented() && self.too_long_with(below) && tail(&below.first_text())
    }
}

/// The web address at cell `index` of `row` that the program broke over
/// rows, whole, or the one that ends its row at the area's edge before a new
/// indented line. `None` for anything else, which the caller reads as before.
pub(super) fn address(
    frame: &FrameData,
    row: u16,
    index: usize,
    columns: Range<u16>,
    rows: Range<u16>,
) -> Option<RowLink> {
    let left = columns.start;
    let area = Area::new(frame, columns, rows);
    let read = |row: u16| area.cells(row).map(Row::new);
    // The rows around `row` that go on from one another, at most
    // `MAX_WRAPPED_ROWS`, with the frame row of the first.
    let mut chain = vec![read(row)?];
    let mut first = row;
    while chain.len() < usize::from(MAX_WRAPPED_ROWS)
        && let Some(above) = first.checked_sub(1).and_then(read)
        && above.joins(&chain[0])
    {
        chain.insert(0, above);
        first -= 1;
    }
    let mut last = row;
    while chain.len() < usize::from(MAX_WRAPPED_ROWS)
        && let Some(below) = last.checked_add(1).and_then(read)
        && chain.last().is_some_and(|above| above.joins(&below))
    {
        chain.push(below);
        last += 1;
    }
    let below = last.checked_add(1).and_then(read);

    // The rows as one text, without the indent of each row after the first,
    // and the byte where each cell's text starts.
    let mut text = String::new();
    let mut starts: Vec<Vec<Option<usize>>> = Vec::with_capacity(chain.len());
    let mut ends = Vec::with_capacity(chain.len());
    for (at, line) in chain.iter().enumerate() {
        let from = if at == 0 { 0 } else { line.indent };
        let mut cells = vec![None; line.cells.len()];
        for (cell, start) in cells.iter_mut().enumerate().take(line.end).skip(from) {
            *start = Some(text.len());
            if line.continuing[cell] {
                continue;
            }
            let symbol = shown(&line.cells[cell]);
            if text.len() + symbol.len() > MAX_ROW_BYTES {
                return None;
            }
            text.push_str(symbol);
        }
        starts.push(cells);
        ends.push(text.len());
    }
    let hit = (*starts.get(usize::from(row - first))?.get(index)?)?;
    let (range, open) = plain_url(&text, hit)?;
    let RowTarget::Web(url) = plain_address(text.get(range.clone())?)? else {
        return None;
    };

    let mut segments =
        (first..=last)
            .zip(chain.iter().zip(&starts))
            .filter_map(|(row, (line, starts))| {
                let on = |cell: &usize| {
                    !line.continuing[*cell] && starts[*cell].is_some_and(|at| range.contains(&at))
                };
                let from = (0..line.end).find(on)?;
                let to = (0..line.end).rev().find(on)?;
                // A wide character's continuation cells belong to the link too.
                let end = (to + 1..line.end)
                    .find(|&cell| !line.continuing[cell])
                    .unwrap_or(line.end);
                // Cell indexes fit the row, whose width is a `u16`.
                Some((row, left + from as u16..left + end as u16))
            });
    let (start_row, start_columns) = segments.next()?;
    let continued: Vec<_> = segments.collect();
    let ending = chain.last()?;
    if continued.is_empty() {
        // An address on one row is the caller's to read, but for the one
        // that ends at the area's edge, which the caller takes to go on in
        // the row below. Before an indented line whose first word would have
        // fit beside it, or is no rest of an address, the program ended the
        // address there.
        let alone = open
            && start_row == last
            && ending.end == ending.cells.len()
            && below
                .as_ref()
                .is_some_and(|below| below.indented() && !ending.joins(below));
        return alone.then_some(RowLink {
            target: RowTarget::Web(url),
            row: start_row,
            columns: start_columns,
            continued,
        });
    }
    // An address that fills the last row read may go on below what is shown
    // or what is read: a destination is never guessed.
    if open && ending.full() && below.as_ref().is_none_or(|below| ending.joins(below)) {
        return None;
    }
    // The host stands whole on the first row, so that a break which was
    // misread can change the path that opens and never the site.
    let line = usize::from(start_row - first);
    let head = text.get(range.start..range.end.min(*ends.get(line)?))?;
    let (_, rest) = head.split_once("://")?;
    if !rest.contains(['/', '?', '#']) {
        return None;
    }
    Some(RowLink {
        target: RowTarget::Web(url),
        row: start_row,
        columns: start_columns,
        continued,
    })
}
