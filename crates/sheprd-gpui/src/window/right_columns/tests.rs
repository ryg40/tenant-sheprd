// Not a glob: the parent's `gpui::*` would shadow `#[test]`.
use super::{Column, MIN_CONTENT, MIN_PANEL, widths};

fn open(wanted: f32) -> Option<Column> {
    Some(Column { wanted, shown: 1. })
}

/// Both columns show in windows from far too narrow to wide: the terminals
/// keep their least width, no column is drawn narrower than its least, and
/// the second column is the one that yields.
#[test]
fn two_columns_leave_the_terminals_their_least_width() {
    for beside in (0..1400).step_by(20).map(|beside| beside as f32) {
        let [first, second] = widths(beside, [open(280.), open(300.)]);
        let taken = first.unwrap_or(0.) + second.unwrap_or(0.);
        if taken > 0. {
            assert!(beside - taken >= MIN_CONTENT, "{beside}: {taken} taken");
        }
        for width in [first, second].into_iter().flatten() {
            assert!(width >= MIN_PANEL, "{beside}: a column {width} wide");
        }
        // The second gives up room, and its place, before the first does.
        assert!(second.is_none() || first == Some(280.), "{beside}");
    }
    // A hidden column takes no room from the other.
    assert_eq!(widths(600., [None, open(300.)]), [None, Some(300.)]);
}
