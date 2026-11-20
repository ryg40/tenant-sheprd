//! The icon button chrome shares: a 24 px square holding a line icon,
//! filled on hover. Nothing about it moves or resizes on hover.

use crate::config::{Theme, corners};
use gpui::{prelude::*, *};

/// The button's side.
pub(crate) const SIZE: f32 = 24.;

/// The corners of the desktop layout's icon buttons, in the sidebar, the
/// tab strips, and the title bar.
pub(crate) const RADIUS: f32 = corners::SMALL + 2.;

/// The icon's side inside it.
const ICON: f32 = 14.;

/// A square with `radius` corners that fills on hover, for the icon the
/// caller puts in it. An icon alone says nothing to a screen reader, so the
/// caller gives the button its role and name, and what a click does.
pub(crate) fn square(id: impl Into<ElementId>, radius: f32, theme: &Theme) -> Stateful<Div> {
    let hover = theme.active;
    div()
        .id(id)
        .flex_none()
        .size(px(SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(radius))
        .hover(move |style| style.bg(rgb(hover)))
        .cursor_pointer()
}

/// The icon at `path` on a [`square`] with `radius` corners.
pub(crate) fn icon_button(
    id: impl Into<ElementId>,
    path: &'static str,
    radius: f32,
    theme: &Theme,
) -> Stateful<Div> {
    square(id, radius, theme).child(crate::icons::icon(path, ICON, theme.subtext()))
}
