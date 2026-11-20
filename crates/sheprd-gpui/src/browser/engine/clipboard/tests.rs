#![allow(clippy::unwrap_used)]
use super::*;
use servo::InputEventAndId;
use std::{collections::HashMap, rc::Rc};

mod copies;

/// The key of the page in these tests.
const PAGE: PageKey = 7;

/// How each read was answered: with its text, or with none when it was
/// dropped, which answers Servo a failure. A read not in it waits.
#[derive(Clone, Default)]
struct Answers(Rc<RefCell<HashMap<usize, Option<String>>>>);

impl Answers {
    fn read(&self, id: usize) -> Read {
        Read {
            id,
            text: None,
            answers: self.clone(),
        }
    }

    fn of(&self, id: usize) -> Option<Option<String>> {
        self.0.borrow().get(&id).cloned()
    }
}

/// Stands in for Servo's request: answered once, as it drops.
struct Read {
    id: usize,
    text: Option<String>,
    answers: Answers,
}

impl Drop for Read {
    fn drop(&mut self) {
        let text = self.text.take();
        self.answers.0.borrow_mut().insert(self.id, text);
    }
}

/// A page that is shown and has the keyboard.
fn focused_page() -> (Arc<Mailbox>, Clipboard<Read>) {
    let mailbox = Arc::new(Mailbox::default());
    let clipboard = Clipboard::new(mailbox.clone(), Arc::default());
    clipboard.shown(true);
    clipboard.focused(true);
    (mailbox, clipboard)
}

/// The key the window sends a page for `keystroke`, through its mapping.
fn key(keystroke: &'static str) -> InputEvent {
    crate::browser::engine::keys::down(&gpui::KeyDownEvent {
        keystroke: gpui::Keystroke::parse(keystroke).unwrap(),
        is_held: false,
        prefer_character_input: false,
    })
}

/// What the window and the engine do as the window sends the page
/// `keystroke` at `now`, and the engine hands it to Servo at once. Returns
/// the key as Servo names it.
fn press(clipboard: &Clipboard<Read>, keystroke: &'static str, now: Instant) -> InputEventId {
    let event = InputEventAndId::from(key(keystroke));
    let pressed = clipboard.mailbox.clipboard.pressed(&event.event, now);
    clipboard.input(pressed, event.id, now);
    event.id
}

/// Answers `read` with `text`, as Servo's request takes a success.
fn succeed(mut read: Read, text: String) {
    read.text = Some(text);
}

/// What the window does at `now`: answers each read the page handed it
/// with `text`.
fn paste(mailbox: &Mailbox, clipboard: &Clipboard<Read>, text: &str, now: Instant) {
    for (number, _) in mailbox.clipboard.take().reads {
        clipboard.settle(number, Some(text.to_owned()), now, succeed);
    }
}

/// What the window does in one turn at `now` with the system clipboard
/// `system`, as `serve_clipboard` does: the page's write first, then each
/// read, answered from the clipboard.
fn serve(mailbox: &Mailbox, clipboard: &Clipboard<Read>, system: &mut String, now: Instant) {
    let asks = mailbox.clipboard.take();
    if let Some((write, _)) = asks.write {
        *system = match write {
            Write::Text(text) => text,
            Write::Clear => String::new(),
        };
    }
    for (number, _) in asks.reads {
        clipboard.settle(number, Some(system.clone()), now, succeed);
    }
}

#[test]
fn a_page_reads_the_clipboard_only_after_its_own_paste_key() {
    let answers = Answers::default();
    let (mailbox, page) = focused_page();
    let (_, other) = focused_page();
    let now = Instant::now();
    let soon = now + Duration::from_millis(50);
    // A script alone, and the paste key another page got.
    page.read(answers.read(0), now);
    press(&other, "ctrl-v", now);
    page.read(answers.read(1), soon);
    paste(&mailbox, &page, "zebra-umbrella-42", soon);
    assert_eq!(answers.of(0), Some(None));
    assert_eq!(answers.of(1), Some(None));

    press(&page, "ctrl-v", now);
    page.read(answers.read(2), soon);
    paste(&mailbox, &page, "zebra-umbrella-42", soon);
    assert_eq!(answers.of(2), Some(Some("zebra-umbrella-42".to_owned())));
    // One press, one read.
    page.read(answers.read(3), soon);
    paste(&mailbox, &page, "zebra-umbrella-42", soon);
    assert_eq!(answers.of(3), Some(None));
}

#[test]
fn every_read_is_answered_when_the_waiting_place_is_full_or_the_page_closes() {
    let answers = Answers::default();
    let (_mailbox, page) = focused_page();
    let now = Instant::now();
    // The window answers none of them.
    for id in 0..=WAITING {
        press(&page, "ctrl-v", now);
        page.read(answers.read(id), now);
    }
    let waiting = (0..=WAITING).filter(|id| answers.of(*id).is_none());
    assert!(waiting.count() <= WAITING);

    drop(page);
    assert!((0..=WAITING).all(|id| answers.of(id) == Some(None)));
}

#[test]
fn no_grant_outlives_a_load_a_reload_or_a_history_step() {
    let steps = [
        Command::Load(PAGE, "https://example.invalid/next".to_owned()),
        Command::Reload(PAGE),
        Command::History(PAGE, false),
    ];
    let now = Instant::now();
    for step in steps {
        let answers = Answers::default();
        let (mailbox, page) = focused_page();
        // A read that waits for the window, and a paste key not used yet.
        press(&page, "ctrl-v", now);
        page.read(answers.read(0), now);
        press(&page, "ctrl-v", now);
        // What the host does as it takes the step, before Servo hears of it.
        before_command(&step, |key| (key == PAGE).then_some(&page));
        assert_eq!(answers.of(0), Some(None));

        page.read(answers.read(1), now);
        paste(&mailbox, &page, "kiwi-lantern-7", now);
        assert_eq!(answers.of(1), Some(None));
    }
}

#[test]
fn a_waiting_read_fails_at_its_deadline_and_a_late_or_older_answer_is_dropped() {
    let answers = Answers::default();
    let (mailbox, page) = focused_page();
    let now = Instant::now();
    // No read waits: the engine sets no timer.
    assert_eq!(page.deadline(), None);

    // No window answers: the engine fails the read at its deadline.
    press(&page, "ctrl-v", now);
    page.read(answers.read(0), now);
    let deadline = page.deadline().unwrap();
    page.expire(deadline - Duration::from_millis(1));
    assert_eq!(answers.of(0), None);
    page.expire(deadline);
    assert_eq!(answers.of(0), Some(None));
    assert_eq!(page.deadline(), None);
    assert!(mailbox.clipboard.take().reads.is_empty());

    // The window's answer arrives after the deadline.
    press(&page, "ctrl-v", now);
    page.read(answers.read(1), now);
    paste(&mailbox, &page, "orbit-pepper-19", deadline);
    assert_eq!(answers.of(1), Some(None));

    // The window took the read, then its grants ended (the window stopped
    // being the active one): its answer, in time, is for an older
    // generation.
    press(&page, "ctrl-v", now);
    page.read(answers.read(2), now);
    let taken = mailbox.clipboard.take();
    mailbox.clipboard.revoke();
    for (number, _) in taken.reads {
        page.settle(number, Some("orbit-pepper-19".to_owned()), now, succeed);
    }
    assert_eq!(answers.of(2), Some(None));
}

#[test]
fn a_cut_reaches_the_window_before_the_paste_after_it_and_no_copy_is_lost() {
    let answers = Answers::default();
    let (mailbox, page) = focused_page();
    let now = Instant::now();
    let mut system = "ember-1204-sentinel".to_owned();
    // Ctrl+X, then Ctrl+V at once: Servo cuts, then waits for the paste's
    // text, and reports neither key handled before it has it.
    let cut = press(&page, "ctrl-x", now);
    let pasted = press(&page, "ctrl-v", now);
    page.write(Write::Text("velvet-anchor-58".to_owned()), now);
    page.read(answers.read(0), now);
    serve(&mailbox, &page, &mut system, now);
    assert_eq!(answers.of(0), Some(Some("velvet-anchor-58".to_owned())));
    page.handled(cut, now);
    page.handled(pasted, now);
    serve(&mailbox, &page, &mut system, now);
    assert_eq!(system, "velvet-anchor-58");

    // Two copies at once; the page fills the first one's copy event with
    // too much text, after the clear Servo sends first.
    let first = press(&page, "ctrl-c", now);
    let second = press(&page, "ctrl-c", now);
    page.write(Write::Clear, now);
    page.write(Write::Text("x".repeat(TEXT + 1)), now);
    serve(&mailbox, &page, &mut system, now);
    assert_eq!(system, "velvet-anchor-58");
    page.write(Write::Text("maple-signal-31".to_owned()), now);
    page.handled(first, now);
    page.handled(second, now);
    serve(&mailbox, &page, &mut system, now);
    assert_eq!(system, "maple-signal-31");

    // A clear with no text after it empties the clipboard once Servo
    // reports its key handled.
    let cleared = press(&page, "ctrl-c", now);
    page.write(Write::Clear, now);
    page.handled(cleared, now);
    serve(&mailbox, &page, &mut system, now);
    assert_eq!(system, "");
}

#[test]
fn a_clipboard_key_sent_before_a_revocation_gets_no_grant() {
    let answers = Answers::default();
    let (mailbox, page) = focused_page();
    let now = Instant::now();
    // The window sends Ctrl+V, then a load, which overtakes the key in the
    // engine.
    let event = InputEventAndId::from(key("ctrl-v"));
    let pressed = mailbox.clipboard.pressed(&event.event, now);
    let load = Command::Load(PAGE, "https://example.invalid/next".to_owned());
    before_command(&load, |key| (key == PAGE).then_some(&page));
    page.input(pressed, event.id, now);
    page.read(answers.read(0), now);
    paste(&mailbox, &page, "harbor-quartz-63", now);
    assert_eq!(answers.of(0), Some(None));

    // The next Ctrl+V, sent after the load, gets its grant.
    press(&page, "ctrl-v", now);
    page.read(answers.read(1), now);
    paste(&mailbox, &page, "harbor-quartz-63", now);
    assert_eq!(answers.of(1), Some(Some("harbor-quartz-63".to_owned())));
}
