#![allow(clippy::unwrap_used)]
use super::*;
use crate::browser::engine::keys;

mod focus_loss;
mod other_page;

/// The modifiers Servo gives the next event after `events` reached it, by
/// its constellation's own rule (`update_active_keybord_modifiers` at the
/// pin) for the four keys a window holds: a key event sets the modifiers it
/// carries, and a modifier key then adds or removes itself. Recheck it, and
/// [`super::modifier`], at each change of the pin.
fn servo_set(events: &[InputEvent]) -> Modifiers {
    let mut set = Modifiers::empty();
    for event in events {
        let InputEvent::Keyboard(key) = event else {
            continue;
        };
        set = key.event.modifiers;
        let changed = match &key.event.key {
            Key::Named(NamedKey::Alt) => Modifiers::ALT,
            Key::Named(NamedKey::Control) => Modifiers::CONTROL,
            Key::Named(NamedKey::Meta) => Modifiers::META,
            Key::Named(NamedKey::Shift) => Modifiers::SHIFT,
            _ => continue,
        };
        set.set(changed, key.event.state == KeyState::Down);
    }
    set
}
