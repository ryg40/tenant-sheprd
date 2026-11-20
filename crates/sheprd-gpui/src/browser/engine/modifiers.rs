//! The modifier keys Servo counts as held.
//!
//! Servo keeps one set of held modifier keys for all its pages: its
//! constellation's `active_keyboard_modifiers`. Every press, release, move,
//! and wheel turn that it hands a page carries that set, and a click on a
//! link opens a new window while the set holds Ctrl
//! (`alternate_action_keyboard_modifier_active` in Servo's
//! `document_event_handler.rs`). Only a key event changes the set: Servo
//! takes the modifiers the event carries, and then adds or removes the key
//! itself when it is a modifier key (`update_active_keybord_modifiers` in
//! Servo's `constellation.rs`).
//!
//! GPUI on Linux sends no key event for a modifier key, only a change of
//! the keys held. So after Ctrl+C in a page, the set kept Ctrl, which `C`
//! carried, until the next key without Ctrl, and the next plain click on a
//! link asked for a new window.
//!
//! The engine keeps the same set by the same rule, and brings Servo's set
//! to the keys that are really held with the key-down or the key-up of each
//! modifier key that differs. Only the page that has the keyboard hears
//! these, and only while no list of its `<select>` is open, which keeps its
//! own key handling. The window says which keys are held as the user
//! presses or lets go of one there, with each press, release, and wheel
//! turn of that page in the same command, and as none when the page loses
//! the keyboard or its window stops being the active one. A press on a page
//! gives it the keyboard first.
//!
//! A press, a move, or a wheel turn of Servo carries no keys of its own, so
//! the pointer's event for a page without the keyboard would get the keys
//! that the page with the keyboard holds: a Ctrl held there would make a
//! wheel turn over another page a Ctrl+wheel. So the page with the keyboard
//! hears its keys go up first, and that page alone; the next input that it
//! takes brings them down again. A page without the keyboard hears no key.
//!
//! The window knows which modifiers are held, not on which side of the
//! keyboard: these key events name no physical key (`Code::Unidentified`)
//! and no side (`Location::Standard`).
//!
//! Servo at the pin keeps the order: it tests a press's place in
//! `WebView::notify_input_event` before it returns, and then sends the press
//! to its constellation on the channel that keys take
//! (`embedder_to_constellation_sender` is the constellation proxy's sender
//! in Servo's `servo.rs`). So a key sent after a press reaches Servo's set
//! after it, and the document takes each event's own set as it handles it.
use super::PageKey;
use servo::{Code, InputEvent, Key, KeyState, KeyboardEvent, Location, Modifiers, NamedKey};

/// The modifier keys the window sends as held. Servo knows more, which no
/// window can hold.
const KEYS: &[(Modifiers, NamedKey)] = &[
    // keep-sorted start
    (Modifiers::ALT, NamedKey::Alt),
    (Modifiers::CONTROL, NamedKey::Control),
    (Modifiers::META, NamedKey::Meta),
    (Modifiers::SHIFT, NamedKey::Shift),
    // keep-sorted end
];

/// The modifiers Servo counts as held, as the engine follows them.
#[derive(Default)]
pub(super) struct HeldKeys {
    held: Modifiers,
    /// The page that heard the last key: the one that has the keyboard.
    page: Option<PageKey>,
}

impl HeldKeys {
    /// Notes `event`, sent to `page`, by Servo's rule.
    fn sent(&mut self, page: PageKey, event: &InputEvent) {
        let InputEvent::Keyboard(key) = event else {
            return;
        };
        let key = &key.event;
        self.held = key.modifiers;
        if let Some(modifier) = modifier(&key.key) {
            self.held.set(modifier, key.state == KeyState::Down);
        }
        self.page = Some(page);
    }

    /// Brings Servo's set to `held`, the keys held in `page`, which has the
    /// keyboard: `send` hands it the key-down or the key-up of each modifier
    /// key that differs, in turn.
    pub(super) fn settle(
        &mut self,
        page: PageKey,
        held: Modifiers,
        mut send: impl FnMut(PageKey, InputEvent),
    ) {
        self.page = Some(page);
        for &(modifier, key) in KEYS {
            let down = held.contains(modifier);
            if self.held.contains(modifier) == down {
                continue;
            }
            self.held.set(modifier, down);
            let state = if down { KeyState::Down } else { KeyState::Up };
            // As a browser does: the key-down of Ctrl says Ctrl is held,
            // and its key-up that it is not.
            let event = KeyboardEvent::new_without_event(
                state,
                Key::Named(key),
                Code::Unidentified,
                Location::Standard,
                self.held,
                false,
                false,
            );
            send(page, InputEvent::Keyboard(event));
        }
    }

    /// Hands `send` the keys that Servo's set needs before `event` reaches
    /// `page`, and notes `event`. `held` goes with the input of the page
    /// that has the keyboard. The pointer's event for another page finds no
    /// key held: the page with the keyboard hears its keys go up.
    pub(super) fn before(
        &mut self,
        page: PageKey,
        held: Option<Modifiers>,
        event: &InputEvent,
        send: impl FnMut(PageKey, InputEvent),
    ) {
        if let Some(held) = held {
            self.settle(page, held, send);
        } else if let Some(owner) = self.page.filter(|owner| *owner != page && pointer(event)) {
            self.settle(owner, Modifiers::empty(), send);
        }
        self.sent(page, event);
    }

    /// Before `page` closes: when it has the keyboard, it hears its keys go
    /// up, as Servo keeps them for every other page.
    pub(super) fn close(&mut self, page: PageKey, send: impl FnMut(PageKey, InputEvent)) {
        if self.page == Some(page) {
            self.settle(page, Modifiers::empty(), send);
            self.page = None;
        }
    }
}

/// Whether `event` is the pointer's, which goes to the page under it whether
/// that page has the keyboard or not.
fn pointer(event: &InputEvent) -> bool {
    matches!(
        event,
        InputEvent::MouseButton(_)
            | InputEvent::MouseLeftViewport(_)
            | InputEvent::MouseMove(_)
            | InputEvent::Touch(_)
            | InputEvent::Wheel(_)
    )
}

/// The modifier that `key` changes in Servo's set, for the keys that a
/// window sends: [`KEYS`], and Caps Lock.
fn modifier(key: &Key) -> Option<Modifiers> {
    let Key::Named(key) = key else {
        return None;
    };
    match key {
        NamedKey::CapsLock => Some(Modifiers::CAPS_LOCK),
        key => KEYS
            .iter()
            .find_map(|&(modifier, named)| (named == *key).then_some(modifier)),
    }
}

#[cfg(test)]
mod tests;
