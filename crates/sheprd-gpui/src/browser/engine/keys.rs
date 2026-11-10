//! GPUI's keys as Servo's. A key that types a character stays that
//! character with Ctrl or Alt held, so the page sees Ctrl+A, Ctrl+C, and
//! Ctrl+V as what they are.
use gpui::{KeyDownEvent, KeyUpEvent, Keystroke};
use servo::{Code, InputEvent, Key, KeyState, KeyboardEvent, Location, Modifiers, NamedKey};

pub(in crate::browser) fn down(event: &KeyDownEvent) -> InputEvent {
    input(&event.keystroke, KeyState::Down, event.is_held)
}

pub(in crate::browser) fn up(event: &KeyUpEvent) -> InputEvent {
    input(&event.keystroke, KeyState::Up, false)
}

fn input(keystroke: &Keystroke, state: KeyState, repeat: bool) -> InputEvent {
    let held = keystroke.modifiers;
    let mut modifiers = Modifiers::empty();
    modifiers.set(Modifiers::CONTROL, held.control);
    modifiers.set(Modifiers::SHIFT, held.shift);
    modifiers.set(Modifiers::ALT, held.alt);
    modifiers.set(Modifiers::META, held.platform);
    InputEvent::Keyboard(KeyboardEvent::new_without_event(
        state,
        key(keystroke),
        code(&keystroke.key),
        Location::Standard,
        modifiers,
        repeat,
        false,
    ))
}

fn key(keystroke: &Keystroke) -> Key {
    let held = keystroke.modifiers;
    // What the key typed, as with Shift: "A" for shift-a.
    if !held.control
        && !held.alt
        && let Some(typed) = keystroke
            .key_char
            .as_ref()
            .filter(|typed| !typed.is_empty())
    {
        return Key::Character(typed.clone());
    }
    if keystroke.key.chars().count() == 1 {
        return Key::Character(keystroke.key.clone());
    }
    Key::Named(match keystroke.key.as_str() {
        "space" => return Key::Character(" ".to_owned()),
        "alt" => NamedKey::Alt,
        "backspace" => NamedKey::Backspace,
        "capslock" => NamedKey::CapsLock,
        "control" => NamedKey::Control,
        "delete" => NamedKey::Delete,
        "down" => NamedKey::ArrowDown,
        "end" => NamedKey::End,
        "enter" => NamedKey::Enter,
        "escape" => NamedKey::Escape,
        "f1" => NamedKey::F1,
        "f2" => NamedKey::F2,
        "f3" => NamedKey::F3,
        "f4" => NamedKey::F4,
        "f5" => NamedKey::F5,
        "f6" => NamedKey::F6,
        "f7" => NamedKey::F7,
        "f8" => NamedKey::F8,
        "f9" => NamedKey::F9,
        "f10" => NamedKey::F10,
        "f11" => NamedKey::F11,
        "f12" => NamedKey::F12,
        "home" => NamedKey::Home,
        "insert" => NamedKey::Insert,
        "left" => NamedKey::ArrowLeft,
        "pagedown" => NamedKey::PageDown,
        "pageup" => NamedKey::PageUp,
        "platform" => NamedKey::Meta,
        "right" => NamedKey::ArrowRight,
        "shift" => NamedKey::Shift,
        "tab" => NamedKey::Tab,
        "up" => NamedKey::ArrowUp,
        _ => NamedKey::Unidentified,
    })
}

/// The physical key, for the keys of a US layout GPUI names.
fn code(key: &str) -> Code {
    match key {
        "a" => Code::KeyA,
        "b" => Code::KeyB,
        "c" => Code::KeyC,
        "d" => Code::KeyD,
        "e" => Code::KeyE,
        "f" => Code::KeyF,
        "g" => Code::KeyG,
        "h" => Code::KeyH,
        "i" => Code::KeyI,
        "j" => Code::KeyJ,
        "k" => Code::KeyK,
        "l" => Code::KeyL,
        "m" => Code::KeyM,
        "n" => Code::KeyN,
        "o" => Code::KeyO,
        "p" => Code::KeyP,
        "q" => Code::KeyQ,
        "r" => Code::KeyR,
        "s" => Code::KeyS,
        "t" => Code::KeyT,
        "u" => Code::KeyU,
        "v" => Code::KeyV,
        "w" => Code::KeyW,
        "x" => Code::KeyX,
        "y" => Code::KeyY,
        "z" => Code::KeyZ,
        "0" => Code::Digit0,
        "1" => Code::Digit1,
        "2" => Code::Digit2,
        "3" => Code::Digit3,
        "4" => Code::Digit4,
        "5" => Code::Digit5,
        "6" => Code::Digit6,
        "7" => Code::Digit7,
        "8" => Code::Digit8,
        "9" => Code::Digit9,
        "`" => Code::Backquote,
        "-" => Code::Minus,
        "=" => Code::Equal,
        "[" => Code::BracketLeft,
        "]" => Code::BracketRight,
        "\\" => Code::Backslash,
        ";" => Code::Semicolon,
        "'" => Code::Quote,
        "," => Code::Comma,
        "." => Code::Period,
        "/" => Code::Slash,
        "alt" => Code::AltLeft,
        "backspace" => Code::Backspace,
        "capslock" => Code::CapsLock,
        "control" => Code::ControlLeft,
        "delete" => Code::Delete,
        "down" => Code::ArrowDown,
        "end" => Code::End,
        "enter" => Code::Enter,
        "escape" => Code::Escape,
        "f1" => Code::F1,
        "f2" => Code::F2,
        "f3" => Code::F3,
        "f4" => Code::F4,
        "f5" => Code::F5,
        "f6" => Code::F6,
        "f7" => Code::F7,
        "f8" => Code::F8,
        "f9" => Code::F9,
        "f10" => Code::F10,
        "f11" => Code::F11,
        "f12" => Code::F12,
        "home" => Code::Home,
        "insert" => Code::Insert,
        "left" => Code::ArrowLeft,
        "pagedown" => Code::PageDown,
        "pageup" => Code::PageUp,
        "platform" => Code::MetaLeft,
        "right" => Code::ArrowRight,
        "shift" => Code::ShiftLeft,
        "space" => Code::Space,
        "tab" => Code::Tab,
        "up" => Code::ArrowUp,
        _ => Code::Unidentified,
    }
}
