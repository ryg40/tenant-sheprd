#![allow(clippy::unwrap_used)]
use super::*;
use std::rc::Rc;

fn option(id: usize, label: &str) -> SelectElementOption {
    SelectElementOption {
        id,
        label: label.to_owned(),
        is_disabled: false,
    }
}

#[test]
fn a_page_with_more_options_than_the_bound_shows_its_first_ones() {
    let options: Vec<SelectElementOptionOrOptgroup> =
        (0..ROWS * 3).map(|id| option(id, "x").into()).collect();
    let rows = rows(&options);
    assert_eq!(rows.len(), ROWS);
    assert!(matches!(rows.last(), Some(Row::Choice { id, .. }) if *id == ROWS - 1));
}

#[test]
fn groups_count_against_the_bound_with_their_options() {
    let group = |start: usize| SelectElementOptionOrOptgroup::Optgroup {
        label: "group".to_owned(),
        options: (start..start + ROWS).map(|id| option(id, "x")).collect(),
    };
    let rows = rows(&[group(0), group(ROWS)]);
    assert_eq!(rows.len(), ROWS);
    assert_eq!(rows[0], Row::Group("group".to_owned()));
    assert!(matches!(
        rows[1],
        Row::Choice {
            id: 0,
            grouped: true,
            ..
        }
    ));
}

#[test]
fn a_label_is_one_bounded_line() {
    let long = "é".repeat(LABEL * 4);
    let line = label(&long);
    assert_eq!(line.chars().count(), LABEL + 1);
    assert!(line.ends_with('…'));
    assert_eq!(label("a\nb\tc\u{0}d"), "a b c d");
    // A label cannot reorder its own row, or break it into lines.
    assert_eq!(
        label("a\u{202E}b\u{2066}c\u{200F}d\u{061C}e\u{2028}f\u{2029}g"),
        "abcde f g"
    );
    // What is left out does not count against the bound.
    let padded = "\u{202E}".repeat(LABEL) + "short";
    assert_eq!(label(&padded), "short");
    assert_eq!(label("short"), "short");
}

#[test]
fn a_group_without_a_label_lists_its_options_as_ungrouped() {
    let rows = rows(&[SelectElementOptionOrOptgroup::Optgroup {
        label: " ".to_owned(),
        options: vec![
            option(0, "a"),
            SelectElementOption {
                is_disabled: true,
                ..option(1, "b")
            },
        ],
    }]);
    assert_eq!(
        rows,
        [
            Row::Choice {
                id: 0,
                label: "a".to_owned(),
                disabled: false,
                grouped: false,
            },
            Row::Choice {
                id: 1,
                label: "b".to_owned(),
                disabled: true,
                grouped: false,
            },
        ]
    );
}

#[test]
fn a_list_of_several_keeps_the_choices_it_does_not_show() {
    let options: Vec<SelectElementOptionOrOptgroup> =
        (0..ROWS + 10).map(|id| option(id, "x").into()).collect();
    let last = last(&rows(&options)).unwrap();
    assert_eq!(last, ROWS - 1);
    let selected = [1, 5, ROWS, ROWS + 7];
    // The list marks only what it shows.
    assert_eq!(chosen(&selected, last, true), [1, 5]);
    // The person turns 5 off and 9 on: the two options past the list stay.
    assert_eq!(
        answered(vec![1, 9], &selected, last, true),
        [1, 9, ROWS, ROWS + 7]
    );
    // Nothing chosen in the list clears only what the list shows.
    assert_eq!(
        answered(Vec::new(), &selected, last, true),
        [ROWS, ROWS + 7]
    );
    // A list of one answers its one choice, and shows one at most.
    assert_eq!(answered(vec![3], &[ROWS + 7], last, false), [3]);
    assert_eq!(chosen(&[ROWS + 7], last, false), Vec::<usize>::new());
    assert_eq!(chosen(&selected, last, false), [1]);
    // A group label at the end is not the last option.
    let rows = [
        choice(&option(0, "a"), false),
        Row::Group("group".to_owned()),
    ];
    assert_eq!(super::last(&rows), Some(0));
    assert_eq!(super::last(&[Row::Group("group".to_owned())]), None);
}

/// Stands for Servo's request: counts its answers, as a request answers
/// when it is dropped.
struct Request(u32, Rc<Cell<u32>>);

impl Drop for Request {
    fn drop(&mut self) {
        self.1.set(self.1.get() + 1);
    }
}

#[test]
fn a_held_request_is_answered_exactly_once() {
    let answers = Rc::new(Cell::new(0));
    let request = |id: u32| Request(id, answers.clone());
    let slot = Slot::default();

    // A choice: the request goes to the caller once, who answers it.
    let first = slot.hold(request(1));
    assert_eq!(answers.get(), 0);
    let taken = slot.take(first);
    assert!(taken.is_some());
    assert!(slot.take(first).is_none());
    assert_eq!(answers.get(), 0);
    drop(taken);
    assert_eq!(answers.get(), 1);

    // A newer control replaces the open one, which is answered.
    let second = slot.hold(request(2));
    let third = slot.hold(request(3));
    assert!(first < second && second < third);
    assert_eq!(answers.get(), 2);

    // A stale number and another control's name leave the open one open.
    assert!(slot.take(second).is_none());
    assert!(slot.take(third + 1).is_none());
    assert!(!slot.close(|request| request.0 == 2));
    assert_eq!(answers.get(), 2);

    // A dismissal answers it once, and a second one finds nothing.
    assert!(slot.close(|request| request.0 == 3));
    assert!(!slot.close(|_| true));
    assert!(slot.take(third).is_none());
    assert_eq!(answers.get(), 3);

    // A tab that closes with a list open answers it.
    let fourth = slot.hold(request(4));
    assert!(fourth > third);
    drop(slot);
    assert_eq!(answers.get(), 4);
}
