#![allow(clippy::unwrap_used)]
use super::*;
use gpui::Modifiers;

fn choice(id: usize, label: &str) -> Row {
    Row::Choice {
        id,
        label: label.to_owned(),
        disabled: false,
        grouped: false,
    }
}

fn disabled(id: usize, label: &str) -> Row {
    Row::Choice {
        id,
        label: label.to_owned(),
        disabled: true,
        grouped: false,
    }
}

fn list(rows: Vec<Row>, chosen: &[usize], multiple: bool) -> List {
    List::new(Arc::new(Menu {
        number: 1,
        control: [0, 0, 100, 20],
        rows,
        chosen: chosen.to_vec(),
        multiple,
    }))
}

/// Rows 0 to 5: a group label, then options whose names for Servo differ
/// from their rows.
fn fruit(chosen: &[usize], multiple: bool) -> List {
    list(
        vec![
            Row::Group("Fruit".to_owned()),
            choice(0, "Apple"),
            disabled(1, "Apricot"),
            choice(2, "Banana"),
            choice(3, "Blueberry"),
            choice(4, "New York"),
        ],
        chosen,
        multiple,
    )
}

fn key(name: &str) -> Keystroke {
    Keystroke {
        modifiers: Modifiers::default(),
        key: name.to_owned(),
        key_char: None,
    }
}

fn typed(text: &str) -> Keystroke {
    Keystroke {
        key_char: Some(text.to_owned()),
        ..key(if text == " " { "space" } else { text })
    }
}

#[test]
fn the_keys_never_rest_on_a_row_nobody_may_choose() {
    let mut list = fruit(&[], false);
    let now = Instant::now();
    let mut seen = Vec::new();
    for name in [
        "down", "down", "up", "home", "end", "pageup", "pagedown", "up", "up", "up",
    ] {
        assert_eq!(list.key(&key(name), now), Outcome::Open);
        seen.push(list.at.unwrap());
    }
    assert!(
        seen.iter().all(|row| list.option(*row).is_some()),
        "{seen:?}"
    );
    assert_eq!(seen, [1, 3, 1, 1, 5, 1, 5, 4, 3, 1]);
}

#[test]
fn enter_answers_the_options_name_and_not_its_row() {
    let mut list = fruit(&[0], false);
    let now = Instant::now();
    assert_eq!(list.at, Some(1));
    list.key(&key("down"), now);
    assert_eq!(list.key(&key("enter"), now), Outcome::Closed(Some(vec![2])));
}

#[test]
fn closing_a_list_of_one_without_a_choice_changes_nothing() {
    let now = Instant::now();
    for name in ["escape", "tab"] {
        let mut list = fruit(&[3], false);
        list.key(&key("up"), now);
        assert_eq!(list.key(&key(name), now), Outcome::Closed(None), "{name}");
    }
    // Enter with the keys on no row, and a press on a row nobody may choose.
    let mut list = fruit(&[], false);
    assert_eq!(list.key(&key("enter"), now), Outcome::Closed(None));
    assert_eq!(list.choose(0), Outcome::Open);
    assert_eq!(list.choose(2), Outcome::Open);
    assert_eq!(list.choose(99), Outcome::Open);
    assert_eq!(list.leave(), None);
}

#[test]
fn a_list_of_several_keeps_each_choice_until_it_is_left() {
    let mut list = fruit(&[0], true);
    let now = Instant::now();
    assert_eq!(list.choose(3), Outcome::Open);
    assert_eq!(list.choose(1), Outcome::Open);
    assert_eq!(list.picked, [2]);
    // From Apple past Apricot, which is off, and Banana to Blueberry.
    list.key(&key("down"), now);
    list.key(&key("down"), now);
    assert_eq!(list.key(&typed(" "), now), Outcome::Open);
    assert_eq!(list.picked, [2, 3]);
    // Escape gives the choices up; Enter and a press outside keep them.
    assert_eq!(list.key(&key("escape"), now), Outcome::Closed(None));
    assert_eq!(
        list.key(&key("enter"), now),
        Outcome::Closed(Some(vec![2, 3]))
    );
    assert_eq!(list.leave(), Some(vec![2, 3]));
}

#[test]
fn typed_letters_find_an_option_that_may_be_chosen() {
    let mut list = fruit(&[], false);
    let start = Instant::now();
    // "ap" is Apple: Apricot may not be chosen.
    list.key(&typed("a"), start);
    list.key(&typed("P"), start);
    assert_eq!(list.at, Some(1));
    // After a pause a letter starts a new word, and typed again goes on.
    let later = start + TYPING * 2;
    list.key(&typed("b"), later);
    assert_eq!(list.at, Some(3));
    let later = later + TYPING * 2;
    list.key(&typed("b"), later);
    assert_eq!(list.at, Some(4));
    // A space within a word is part of it, and chooses nothing.
    let later = later + TYPING * 2;
    for letter in ["n", "e", "w", " ", "y"] {
        assert_eq!(list.key(&typed(letter), later), Outcome::Open);
    }
    assert_eq!(list.at, Some(5));
    // A letter with Ctrl held is no letter of a word.
    let later = later + TYPING * 2;
    let mut held = typed("a");
    held.modifiers.control = true;
    list.key(&held, later);
    assert_eq!(list.at, Some(5));
}

fn inside(list: Bounds<Pixels>, page: Size<Pixels>) -> bool {
    list.left() >= px(0.)
        && list.top() >= px(0.)
        && list.right() <= page.width
        && list.bottom() <= page.height
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
}

#[test]
fn a_list_opens_under_its_control_or_over_it_near_the_pages_end() {
    let page = size(px(800.), px(600.));
    let under = place(rect(40., 100., 200., 20.), page, 300);
    assert_eq!(under.top(), px(120.));
    assert_eq!(under.left(), px(40.));
    assert_eq!(under.size.width, px(200.));
    assert!(inside(under, page));
    let control = rect(40., 560., 200., 20.);
    let over = place(control, page, 300);
    assert_eq!(over.bottom(), control.top());
    assert!(over.size.height > px(ROW * 5.));
    assert!(inside(over, page));
    // A short list that fits under the control stays there.
    let short = place(control, page, 0);
    assert_eq!(short.top(), control.bottom());
}

#[test]
fn a_list_stays_inside_its_page_whatever_the_page_reports() {
    let page = size(px(300.), px(200.));
    let far = i32::MAX as f32;
    for control in [
        rect(far, far, far, far),
        rect(-far, -far, 10., 10.),
        rect(-far, far, far, 0.),
        rect(far, -far, 0., far),
        rect(0., 0., 0., 0.),
        rect(290., 190., 100., 100.),
    ] {
        for rows in [0, 1, 7, 100_000] {
            let list = place(control, page, rows);
            assert!(inside(list, page), "{control:?} {rows} {list:?}");
        }
    }
    // A page with no room at all gets a list with no size, not a panic.
    let none = place(rect(5., 5., 5., 5.), size(px(0.), px(0.)), 3);
    assert_eq!(none.size.height, px(0.));
}

#[test]
fn a_control_in_a_frame_gets_its_list_under_the_press() {
    let menu = Menu {
        number: 1,
        control: [10, 20, 100, 20],
        rows: Vec::new(),
        chosen: Vec::new(),
        multiple: false,
    };
    // The press is in the box Servo reports, at the page's zoom.
    let top = control(&menu, 2., Some(point(px(30.), px(50.))));
    assert_eq!(top, rect(20., 40., 200., 40.));
    assert_eq!(control(&menu, 1., None), rect(10., 20., 100., 20.));
    // The press is elsewhere: the box is a frame's own.
    let framed = control(&menu, 1., Some(point(px(400.), px(300.))));
    assert_eq!(framed.left(), px(400.));
    assert!(framed.top() > px(300.));
}

#[test]
fn an_answer_the_engine_did_not_take_is_sent_again() {
    let owed = Owed::default();
    let sent = RefCell::new(Vec::new());
    let full = |answer: &Answer| {
        sent.borrow_mut().push(answer.clone());
        false
    };
    owed.give((1, Some(vec![4])), full);
    owed.settle(full);
    assert_eq!(*sent.borrow(), [(1, Some(vec![4])), (1, Some(vec![4]))]);
    // Taken at last, it is not sent a second time.
    let taken = |answer: &Answer| {
        sent.borrow_mut().push(answer.clone());
        true
    };
    owed.settle(taken);
    owed.settle(taken);
    assert_eq!(sent.borrow().len(), 3);
    // A later list's answer replaces one still owed.
    owed.give((2, None), full);
    owed.give((3, Some(vec![0])), taken);
    owed.settle(taken);
    assert_eq!(sent.borrow()[3..], [(2, None), (3, Some(vec![0]))]);
}

#[test]
fn the_page_hears_no_release_or_repeat_of_a_key_the_list_took() {
    let taken = Taken::default();
    taken.took("escape");
    // Held on after the list closed: its repeats are not the page's.
    assert!(taken.repeats("escape", true));
    assert!(!taken.repeats("a", true));
    assert!(taken.released("escape"));
    // The page hears the next press of the key, and its release.
    assert!(!taken.released("escape"));
    assert!(!taken.repeats("escape", false));
    // A release that never came does not take the next press's release.
    taken.took("enter");
    assert!(!taken.repeats("enter", false));
    assert!(!taken.released("enter"));
    // However many keys a list takes, it remembers a bounded number.
    for key in 0..TAKEN * 4 {
        taken.took(&key.to_string());
    }
    assert_eq!(taken.0.borrow().len(), TAKEN);
}
