//! A page in the window, and the element that lays it out where its group
//! draws. Two backends draw one: wry's native view on macOS and Windows
//! (`webview`), and Servo on Linux with the `servo` feature (`offscreen`), which
//! draws offscreen into an image the window paints itself.
use {
    crate::browser::TabId,
    gpui::{Bounds, Pixels, point, px},
    std::{cell::RefCell, collections::HashMap, rc::Rc},
};

#[cfg(servo_pages)]
mod offscreen;
#[cfg(any(target_os = "macos", windows))]
mod webview;

#[cfg(servo_pages)]
pub(crate) use offscreen::{Colors, Page, PageElement, Retired};
#[cfg(any(target_os = "macos", windows))]
pub(crate) use webview::{Page, PageElement};

/// Where each page drew in the frame being drawn, in the window's points.
/// Pages fill it as they lay out, and the next frame starts it afresh.
pub(crate) type Drawn = Rc<RefCell<HashMap<TabId, Bounds<Pixels>>>>;

/// `bounds` with each edge on the nearest whole device pixel at `scale`.
pub(crate) fn snap(bounds: Bounds<Pixels>, scale: f32) -> Bounds<Pixels> {
    if !scale.is_finite() || scale <= 0. {
        return bounds;
    }
    let edge = |value: Pixels| px((f32::from(value) * scale).round() / scale);
    Bounds::from_corners(
        point(edge(bounds.left()), edge(bounds.top())),
        point(edge(bounds.right()), edge(bounds.bottom())),
    )
}

#[cfg(test)]
mod tests;
