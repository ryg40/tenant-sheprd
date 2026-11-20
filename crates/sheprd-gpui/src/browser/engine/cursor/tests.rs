use super::*;
use servo::{DevicePoint, InputEventAndId, MouseMoveEvent, WebViewPoint};

/// The id of a new pointer move, as Servo gives it.
fn moved() -> InputEventId {
    let point = WebViewPoint::Device(DevicePoint::new(1., 1.));
    InputEventAndId::from(InputEvent::MouseMove(MouseMoveEvent::new(point))).id
}

#[test]
fn a_page_gets_its_shapes_and_cannot_hide_the_pointer() {
    assert_eq!(style(Cursor::Pointer), CursorStyle::PointingHand);
    assert_eq!(style(Cursor::Text), CursorStyle::IBeam);
    assert_eq!(style(Cursor::EwResize), CursorStyle::ResizeLeftRight);
    assert_eq!(style(Cursor::None), CursorStyle::Arrow);
}

#[test]
fn a_reset_shows_the_arrow_until_a_move_shows_servos_shape_again() {
    let pointer = Pointer::with(Arc::default(), Arc::default());
    pointer.show(Cursor::Pointer);
    // A new document, or a page shown again.
    pointer.reset();
    assert_eq!(pointer.mailbox.cursor.get(), CursorStyle::Arrow);
    // Servo's shape for the move's place equals the one before the reset,
    // so Servo reports none.
    let moved = moved();
    pointer.sent(moved);
    pointer.handled(moved, InputEventResult::default(), || Cursor::Pointer);
    assert_eq!(pointer.mailbox.cursor.get(), CursorStyle::PointingHand);
}

#[test]
fn a_pointer_that_keeps_moving_shows_servos_shape_after_a_reset() {
    let pointer = Pointer::with(Arc::default(), Arc::default());
    pointer.reset();
    // Servo never reports the first move as handled.
    pointer.sent(moved());
    let later = moved();
    pointer.sent(later);
    // The pointer moves on before Servo has handled the later move.
    pointer.sent(moved());
    pointer.handled(later, InputEventResult::default(), || Cursor::Pointer);
    assert_eq!(pointer.mailbox.cursor.get(), CursorStyle::PointingHand);
}
