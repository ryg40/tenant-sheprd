#![allow(clippy::unwrap_used)]
use super::{AGAIN, Blank, SETTLE, UNANSWERED};
use crate::browser::TabId;
use std::time::Instant;

fn tab(id: u64) -> TabId {
    TabId::test(id)
}

/// What takes a page's answer.
type Answers = Box<dyn Fn(String)>;

/// Asks every page that is due at `now` and hands back what takes each
/// answer.
fn asked(blank: &mut Blank, now: Instant) -> Vec<(TabId, Answers)> {
    let mut due = blank.due(now);
    due.sort();
    due.into_iter()
        .map(|id| (id, Box::new(blank.ask(id)) as Answers))
        .collect()
}

#[test]
fn an_empty_page_shows_the_hint_only_after_it_had_time_to_fill_itself() {
    let mut blank = Blank::default();
    let start = Instant::now();
    assert!(!blank.loaded(tab(1), start));
    // Not asked at once: a script may still fill the page.
    assert!(blank.due(start).is_empty());
    let answers = asked(&mut blank, start + SETTLE);
    assert_eq!(answers.len(), 1);
    assert!(!blank.shows(tab(1)));
    (answers[0].1)("true".to_owned());
    assert!(blank.settle(start + SETTLE));
    assert!(blank.shows(tab(1)));
}

#[test]
fn a_page_with_content_gets_no_hint_and_no_more_questions() {
    let mut blank = Blank::default();
    let start = Instant::now();
    blank.loaded(tab(1), start);
    let answers = asked(&mut blank, start + SETTLE);
    (answers[0].1)("false".to_owned());
    assert!(!blank.settle(start + SETTLE));
    assert!(!blank.shows(tab(1)));
    assert!(blank.due(start + SETTLE + AGAIN * 10).is_empty());
}

#[test]
fn the_hint_goes_when_the_page_fills_itself_later() {
    let mut blank = Blank::default();
    let start = Instant::now();
    blank.loaded(tab(1), start);
    (asked(&mut blank, start + SETTLE)[0].1)("true".to_owned());
    blank.settle(start + SETTLE);
    // Asked again while the hint shows, and not before its time.
    assert!(blank.due(start + SETTLE).is_empty());
    let again = asked(&mut blank, start + SETTLE + AGAIN);
    assert!(blank.shows(tab(1)));
    (again[0].1)("false".to_owned());
    assert!(blank.settle(start + SETTLE + AGAIN));
    assert!(!blank.shows(tab(1)));
}

#[test]
fn an_answer_about_the_page_before_does_not_mark_the_new_page() {
    let mut blank = Blank::default();
    let start = Instant::now();
    blank.loaded(tab(1), start);
    let old = asked(&mut blank, start + SETTLE);
    // The tab loads another page before the first one answers.
    blank.loaded(tab(1), start + SETTLE);
    (old[0].1)("true".to_owned());
    assert!(!blank.settle(start + SETTLE));
    assert!(!blank.shows(tab(1)));
    // A new load takes the hint of the page before it away at once.
    (asked(&mut blank, start + SETTLE * 2)[0].1)("true".to_owned());
    blank.settle(start + SETTLE * 2);
    assert!(blank.shows(tab(1)));
    assert!(blank.loaded(tab(1), start + SETTLE * 3));
    assert!(!blank.shows(tab(1)));
}

#[test]
fn only_the_scripts_own_true_shows_the_hint() {
    let mut blank = Blank::default();
    let start = Instant::now();
    for (id, raw) in [(1, "null"), (2, "\"true\""), (3, "1"), (4, "")] {
        blank.loaded(tab(id), start);
        let answers = asked(&mut blank, start + SETTLE);
        let answer = answers.iter().find(|(asked, _)| *asked == tab(id)).unwrap();
        (answer.1)(raw.to_owned());
        blank.settle(start + SETTLE);
        assert!(!blank.shows(tab(id)), "{raw}");
    }
    // A page that still loads is asked again; the others are done.
    assert_eq!(blank.due(start + SETTLE * 2), [tab(1)]);
}

/// A page cannot keep the window asking: a document that never answers is
/// dropped after a few questions, and a hint it showed goes with it.
#[test]
fn a_page_that_never_answers_is_not_asked_for_ever() {
    let mut blank = Blank::default();
    let start = Instant::now();
    blank.loaded(tab(1), start);
    (asked(&mut blank, start + SETTLE)[0].1)("true".to_owned());
    blank.settle(start + SETTLE);
    assert!(blank.shows(tab(1)));
    let mut now = start + SETTLE;
    let mut questions = 0;
    while let Some((_, answer)) = asked(&mut blank, now + AGAIN).pop() {
        now += AGAIN;
        questions += 1;
        assert!(questions <= UNANSWERED, "asked {questions} times");
        answer("null".to_owned());
        blank.settle(now);
    }
    assert!(!blank.shows(tab(1)));
}
