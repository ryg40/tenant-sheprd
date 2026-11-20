//! Chrome buttons in the desktop layout: the icon buttons of
//! [`chrome_button`], small rounded squares that fill under the pointer, in
//! place of the full-height cells and the drawn sidebar glyph the other
//! layouts keep.

use crate::{SheprdWindow, chrome_button};
use gpui::{prelude::*, *};

/// The room either side of a strip button, so the button with its room
/// takes the 30 px the cell it replaces took.
const STRIP_MARGIN: f32 = 3.;

/// The sidebar toggle's icon.
const TOGGLE_ICON: f32 = 16.;

impl SheprdWindow {
    /// The strip button `id` showing `icon`, an icon button centered in the
    /// strip, for the caller to name and give its action.
    pub(super) fn desktop_strip_button(
        &self,
        id: impl Into<ElementId>,
        icon: &'static str,
    ) -> Stateful<Div> {
        chrome_button::icon_button(id, icon, chrome_button::RADIUS, &self.theme)
            .self_center()
            .mx(px(STRIP_MARGIN))
    }

    /// The sidebar toggle `toggle`, an icon button that carries its
    /// identity and listeners, showing `panel-left.svg`, mirrored right to
    /// left. It stands centered in a slot of `slot` px, the side of the
    /// button that draws the glyph, so the title bar keeps its measures.
    pub(crate) fn desktop_sidebar_toggle(&self, toggle: Stateful<Div>, slot: f32) -> AnyElement {
        div()
            .flex_none()
            .self_center()
            .size(px(slot))
            .flex()
            .items_center()
            .justify_center()
            .child(
                toggle.child(
                    svg()
                        // The panel sits on the side the sidebar does.
                        .path(crate::direction::icon("icons/panel-left.svg"))
                        .size(px(TOGGLE_ICON))
                        .text_color(rgb(self.theme.foreground)),
                ),
            )
            .into_any_element()
    }
}
