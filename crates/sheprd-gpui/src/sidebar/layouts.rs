//! The row layouts the config can pick. Each one is a unit struct assembled
//! from the shared pieces in `parts`, so adding a layout is a new file and a
//! `RowStyle` variant.

mod herdr;
mod minimal;
mod orca;
mod parts;
mod superset;

pub(super) use {herdr::Herdr, minimal::Minimal, orca::Orca, superset::Superset};

use super::{cell::Fold, label_text, layout::FoldMark};
use crate::config::{FontConfig, Theme};
use gpui::{prelude::*, *};

impl Fold {
    /// The fold's triangle, sized by the caller through its text size.
    /// Clicking it never selects the row it sits on.
    pub(super) fn element(self, theme: &Theme) -> Stateful<Div> {
        let glyph = label_text(fold_glyph(self.collapsed));
        self.target(theme).child(glyph)
    }

    /// The fold as a chevron icon `size` pixels across, for a layout whose
    /// folds draw icons rather than text.
    pub(super) fn chevron(self, size: f32, theme: &Theme) -> Stateful<Div> {
        let foreground = theme.foreground;
        let icon = chevron(self.collapsed, size, theme.muted)
            .group_hover(FOLD_GROUP, move |style| style.text_color(rgb(foreground)));
        self.target(theme)
            .group(FOLD_GROUP)
            .flex()
            .justify_center()
            .child(icon)
    }

    fn target(self, theme: &Theme) -> Stateful<Div> {
        let Self {
            id,
            index,
            collapsed: _,
            toggle,
        } = self;
        let (muted, foreground) = (theme.muted, theme.foreground);
        div()
            .id(id)
            .debug_selector(move || format!("collapse-{index}"))
            .flex_none()
            .text_color(rgb(muted))
            .hover(move |style| style.text_color(rgb(foreground)))
            .cursor_pointer()
            .on_click(move |event, window, cx| {
                cx.stop_propagation();
                toggle(event, window, cx);
            })
    }
}

/// A fold chevron's size at the default sidebar font; [`super::scaled`]
/// sizes it for the configured one.
const CHEVRON: f32 = 12.;

/// Group every chevron's fold target joins, so the chevron follows the
/// target's hover as a text triangle follows its text color. GPUI resolves
/// it to the innermost target, so folds can share it.
const FOLD_GROUP: &str = "sidebar-fold";

/// A text fold's triangle: pointing along the line while folded, down
/// while open.
fn fold_glyph(collapsed: bool) -> &'static str {
    if collapsed {
        crate::direction::glyph("\u{25b8}")
    } else {
        "\u{25be}"
    }
}

/// A host or tag heading's fold mark in `color`: the layout's chevron, or
/// its text triangle, which takes the heading's text color instead.
pub(super) fn heading_fold(
    mark: FoldMark,
    collapsed: bool,
    font: &FontConfig,
    color: u32,
) -> AnyElement {
    match mark {
        FoldMark::Glyph => label_text(fold_glyph(collapsed)).into_any_element(),
        FoldMark::Chevron => {
            chevron(collapsed, super::scaled(font, CHEVRON), color).into_any_element()
        }
    }
}

/// A fold's chevron, `size` pixels across in `color`: pointing along the
/// line while folded, down while open.
fn chevron(collapsed: bool, size: f32, color: u32) -> Svg {
    svg()
        .path(if collapsed {
            crate::direction::icon("icons/chevron-right.svg")
        } else {
            "icons/chevron-down.svg"
        })
        .flex_none()
        .size(px(size))
        .text_color(rgb(color))
}
