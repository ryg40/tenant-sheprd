//! Text of a session as it may be shown: control characters removed, and
//! cut at a bound.

/// The most characters of a title.
pub(super) const TITLE: usize = 200;
/// The most characters of each other field.
pub(super) const FIELD: usize = 8_000;
/// The most bytes of an option's text.
pub(super) const OPTION_BYTES: usize = 256 * 1024;

/// Keeps newline and tab; removes every other control character.
fn shown(c: &char) -> bool {
    !c.is_control() || matches!(c, '\n' | '\t')
}

/// At most `max` characters of `text`, without control characters.
pub(super) fn chars(text: &str, max: usize) -> String {
    text.chars().filter(shown).take(max).collect()
}

/// At most `max` bytes of `text`, without control characters, and whether
/// some of it was cut.
pub(super) fn bytes(text: &str, max: usize) -> (String, bool) {
    let mut out = String::with_capacity(text.len().min(max));
    for c in text.chars().filter(shown) {
        if out.len() + c.len_utf8() > max {
            return (out, true);
        }
        out.push(c);
    }
    (out, false)
}
