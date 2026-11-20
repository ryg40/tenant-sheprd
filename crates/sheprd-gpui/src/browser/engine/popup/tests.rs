#![allow(clippy::unwrap_used)]
use super::*;
use servo::{DevicePoint, MouseButton, MouseButtonEvent, WebViewPoint};

mod adoption;
mod grants;

fn point() -> WebViewPoint {
    WebViewPoint::Device(DevicePoint::new(5., 5.))
}

/// The main button going down or up over the page.
fn button(action: MouseButtonAction) -> InputEvent {
    InputEvent::MouseButton(MouseButtonEvent::new(action, MouseButton::Primary, point()))
}

/// `event` as the window sends it at `at` to a page whose grants `waiting`
/// keeps, and as the engine then notes it in the page's `gate`.
fn send(gate: &Activation, waiting: &Waiting, event: &InputEvent, at: Instant) {
    gate.pressed(activation(event, waiting, at));
}
