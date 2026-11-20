//! A proportional face's line of segments, shared as a monospace line shares
//! its glyphs: a segment that fits keeps its whole text, and only the ones
//! that do not fit truncate, each to a fair share of the room the others
//! leave ([`segment_widths`]). Flex shrinking would cut every segment by the
//! same fraction, so a short name collapsed to a letter or two.
//!
//! The row code has no window, so the line measures its texts when GPUI lays
//! it out, with the window's text system. Its line layout cache keeps the
//! shapes of the frame before, so a row laid out again measures from it.

use super::label_text;
use crate::{
    config::FontConfig,
    sidebar::{line_height, segment_widths},
};
use gpui::{prelude::*, *};

/// How a cell of a fair line takes its width.
pub(super) enum Sizing {
    /// Text that shares the line with the other shared texts, in the weight
    /// its cell sets, or the line's own for `None`.
    Share(SharedString, Option<FontWeight>),
    /// A cell that keeps its natural width, such as an icon or a counter:
    /// these pixels plus these texts.
    Keep(f32, Vec<(SharedString, Option<FontWeight>)>),
}

/// The line, `width` across, of `parts`: each cell after its separator,
/// which is drawn in `muted`. Separators and kept cells take their room
/// first; the shared cells divide the rest.
pub(super) fn fair_line(
    parts: impl IntoIterator<Item = (&'static str, Div, Sizing)>,
    width: f32,
    font: &FontConfig,
    muted: u32,
) -> Div {
    div()
        .relative()
        .w(px(width))
        .h(px(line_height(font)))
        .flex_none()
        .overflow_hidden()
        .child(FairLine {
            parts: parts.into_iter().collect(),
            width,
            muted,
        })
}

struct FairLine {
    parts: Vec<(&'static str, Div, Sizing)>,
    width: f32,
    muted: u32,
}

impl FairLine {
    /// Each shared cell's width, measured in the text style the line
    /// inherits, which its parent has pushed by the time it is laid out.
    /// A text takes the whole pixels GPUI's text layout rounds it up to, so
    /// the separators and kept cells leave the shares no less than they say.
    fn shares(&self, window: &Window) -> Vec<f32> {
        let style = window.text_style();
        let size = style.font_size.to_pixels(window.rem_size());
        let measure = |text: &SharedString, weight: Option<FontWeight>| {
            // Names are untrusted text and may hold a line break. The line is
            // one text line tall, so only the first line shows, and
            // `shape_line` takes no break.
            let text = match text.split_once('\n') {
                Some((first, _)) => SharedString::from(first.to_owned()),
                None => text.clone(),
            };
            if text.is_empty() {
                return 0.;
            }
            let mut run = style.to_run(text.len());
            if let Some(weight) = weight {
                run.font.weight = weight;
            }
            f32::from(
                window
                    .text_system()
                    .shape_line(text, size, &[run], None)
                    .width,
            )
            .ceil()
        };
        let mut taken = 0.;
        let mut natural = Vec::with_capacity(self.parts.len());
        for (separator, _, sizing) in &self.parts {
            if !separator.is_empty() {
                taken += measure(&SharedString::new_static(separator), None);
            }
            match sizing {
                Sizing::Share(text, weight) => natural.push(measure(text, *weight)),
                Sizing::Keep(fixed, texts) => {
                    taken += fixed
                        + texts
                            .iter()
                            .map(|(text, weight)| measure(text, *weight))
                            .sum::<f32>();
                }
            }
        }
        segment_widths(&natural, self.width - taken)
    }
}

impl IntoElement for FairLine {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for FairLine {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, AnyElement) {
        let mut shares = self.shares(window).into_iter();
        let mut line = div().flex().size_full();
        for (separator, cell, sizing) in std::mem::take(&mut self.parts) {
            if !separator.is_empty() {
                line = line.child(
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .text_color(rgb(self.muted))
                        .child(label_text(separator)),
                );
            }
            // No cell shrinks: the shares already leave the rest their room.
            line = line.child(
                match sizing {
                    Sizing::Share(..) => cell.w(px(shares.next().unwrap_or_default())),
                    Sizing::Keep(..) => cell,
                }
                .flex_none(),
            );
        }
        let mut line = line.into_any_element();
        (line.request_layout(window, cx), line)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        line: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        line.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        line: &mut AnyElement,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        line.paint(window, cx);
    }
}
