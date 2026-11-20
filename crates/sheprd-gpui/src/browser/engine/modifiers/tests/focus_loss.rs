//! Servo's set of held keys when a page loses the keyboard.
use super::*;

#[test]
fn after_ctrl_c_and_the_loss_of_the_keyboard_servo_holds_no_ctrl() {
    let (mut held, mut servo, page) = (HeldKeys::default(), Vec::new(), 1);
    // The user holds Ctrl in the page and types C, as GPUI reports it.
    held.settle(page, Modifiers::CONTROL, |_, event| servo.push(event));
    let ctrl_c = gpui::Keystroke::parse("ctrl-c").unwrap();
    let down = gpui::KeyDownEvent {
        keystroke: ctrl_c.clone(),
        is_held: false,
        prefer_character_input: false,
    };
    let up = gpui::KeyUpEvent { keystroke: ctrl_c };
    for event in [keys::down(&down), keys::up(&up)] {
        held.sent(page, &event);
        servo.push(event);
    }
    assert!(servo_set(&servo).contains(Modifiers::CONTROL));
    // The page loses the keyboard before Ctrl goes up, and the user lets go
    // of it where the page does not hear.
    held.settle(page, Modifiers::empty(), |_, event| servo.push(event));
    assert!(!servo_set(&servo).contains(Modifiers::CONTROL));
}
