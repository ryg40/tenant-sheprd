//! Servo's set of held keys for the pointer's events over a page that does
//! not have the keyboard.
use super::*;
use servo::{DevicePoint, WebViewPoint, WheelDelta, WheelEvent, WheelMode};

#[test]
fn a_wheel_turn_over_another_page_finds_no_key_held_and_sends_it_no_key() {
    let (mut held, mut servo) = (HeldKeys::default(), Vec::new());
    let (keyboard, other) = (1, 2);
    // The user holds Ctrl in the page that has the keyboard.
    held.settle(keyboard, Modifiers::CONTROL, |page, event| {
        servo.push((page, event));
    });
    // The window sends a wheel turn over the other page without keys.
    let delta = WheelDelta {
        x: 0.,
        y: -40.,
        z: 0.,
        mode: WheelMode::DeltaPixel,
    };
    let point = WebViewPoint::Device(DevicePoint::new(5., 5.));
    let wheel = InputEvent::Wheel(WheelEvent::new(delta, point, Modifiers::empty()));
    held.before(other, None, &wheel, |page, event| servo.push((page, event)));
    let events: Vec<InputEvent> = servo.iter().map(|(_, event)| event.clone()).collect();
    assert_eq!(servo_set(&events), Modifiers::empty());
    let keyed = |page| {
        servo
            .iter()
            .any(|(to, event)| *to == page && matches!(event, InputEvent::Keyboard(_)))
    };
    assert!(!keyed(other));
}
