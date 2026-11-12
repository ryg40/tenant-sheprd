//! The columns on the window's right, beside the terminals: the mod panel,
//! then the file explorer at the window's edge. This is the one place that
//! gives each its width, so together they never take the room the terminals
//! need, and the one place that draws them beside the content.
//!
//! When the window is too narrow for all of it, the mod panel yields first:
//! it narrows to [`MIN_PANEL`] and then steps aside, and only then does the
//! file explorer narrow, and step aside in turn. The explorer is shown by
//! the user's or an agent's own request for it, while the mod panel shows
//! whenever a mod has a section. A column that stepped aside comes back
//! when there is room again; nothing about it is forgotten.

use crate::{SheprdWindow, motion, panel_resize::PanelDrag};
use gpui::{prelude::*, *};

/// The least room the terminals keep beside the columns.
pub(crate) const MIN_CONTENT: f32 = 240.;
/// The narrowest a column is drawn; with less room it steps aside.
pub(crate) const MIN_PANEL: f32 = 160.;

/// A column that asks for room.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Column {
    /// The width it takes when nothing else needs the room.
    pub(crate) wanted: f32,
    /// The share of it that has slid in, which is the room it takes now.
    pub(crate) shown: f32,
}

/// The widths of `columns` in a window with `beside` pixels at the right of
/// the sidebar, `None` for a column that is absent or steps aside. Room is
/// given in the order of `columns`, so the last yields first, and what the
/// columns take together leaves the terminals [`MIN_CONTENT`].
pub(crate) fn widths<const N: usize>(
    beside: f32,
    columns: [Option<Column>; N],
) -> [Option<f32>; N] {
    let mut room = beside - MIN_CONTENT;
    columns.map(|column| {
        let column = column.filter(|column| column.shown > 0.)?;
        let width = column.wanted.min(room);
        if width < MIN_PANEL {
            return None;
        }
        room -= width * column.shown;
        Some(width)
    })
}

/// Each right-hand column's width in this window, `None` while it is not
/// drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Widths {
    pub(crate) files: Option<f32>,
    pub(crate) mods: Option<f32>,
}

/// Clips `panel`, laid out `width` wide, to the share that has slid in,
/// revealed from its left edge.
fn slide(panel: Stateful<Div>, width: f32, shown: f32) -> Div {
    div()
        .relative()
        .flex_none()
        .h_full()
        .w(px(width * shown))
        .overflow_hidden()
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left_0()
                .w(px(width))
                // Narrowed to the room it was given, past the drag's cap.
                .child(panel.w(px(width))),
        )
}

impl SheprdWindow {
    /// The widths the right-hand columns have now, for drawing them and
    /// for the focus regions. Zen mode keeps the terminals alone, as it
    /// does with the sidebar.
    pub(crate) fn right_column_widths(&self) -> Widths {
        if self.zen_layout() {
            return Widths::default();
        }
        let now = motion::now();
        let beside =
            self.viewport_width - self.sidebar_column_width(self.viewport_width).unwrap_or(0.);
        let column = |width: crate::panel_resize::PanelWidth, motion: &motion::Toggle| {
            Some(Column {
                wanted: width.width(beside),
                shown: motion.shown(now),
            })
        };
        let [files, mods] = widths(
            beside,
            [
                column(self.file_tree_width, self.file_tree.motion()),
                column(self.mod_panel_width, self.mods.panel_motion()),
            ],
        );
        Widths { files, mods }
    }

    /// `content` with the columns that show beside it on the right.
    pub(crate) fn with_right_columns(
        &self,
        content: AnyElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Widths { files, mods } = self.right_column_widths();
        if files.is_none() && mods.is_none() {
            return content;
        }
        let now = motion::now();
        let mods = mods.map(|width| {
            let motion = self.mods.panel_motion();
            if motion.moving(now) {
                window.request_animation_frame();
            }
            let panel = self.resizable_panel(
                self.render_mod_panel(cx),
                "mod-panel-resize",
                PanelDrag::ModPanel,
                None,
                cx,
            );
            slide(panel, width, motion.shown(now))
        });
        let files = files.map(|width| {
            let motion = self.file_tree.motion();
            if motion.moving(now) {
                window.request_animation_frame();
            }
            // A cheap compare while nothing changed, as the sidebar's
            // field does, so a new theme reaches the field while it shows.
            let (font, theme) = (self.config.ui.clone(), self.theme.clone());
            self.file_tree
                .search()
                .update(cx, |input, cx| input.set_appearance(font, theme, cx));
            let panel = self.resizable_panel(
                self.render_file_tree(cx),
                "file-tree-resize",
                PanelDrag::FileTree,
                None,
                cx,
            );
            slide(panel, width, motion.shown(now))
        });
        div()
            .flex()
            .flex_1()
            .size_full()
            .min_w_0()
            .min_h_0()
            .child(div().flex().flex_1().min_w_0().min_h_0().child(content))
            .children(mods)
            .children(files)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
