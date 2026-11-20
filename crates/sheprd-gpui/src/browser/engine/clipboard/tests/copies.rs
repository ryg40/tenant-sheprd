//! One copy or cut of a page is one change of the clipboard.
use super::*;

/// What a paste in the page gets at `now`, after the window did what waits
/// with the system clipboard `system`.
fn pasted(
    mailbox: &Mailbox,
    page: &Clipboard<Read>,
    system: &mut String,
    now: Instant,
) -> Option<Option<String>> {
    let answers = Answers::default();
    let key = press(page, "ctrl-v", now);
    page.read(answers.read(0), now);
    serve(mailbox, page, system, now);
    page.handled(key, now);
    answers.of(0)
}

#[test]
fn a_copy_that_only_clears_counts_before_an_oversized_copy_after_it() {
    let (mailbox, page) = focused_page();
    let now = Instant::now();
    let mut system = "cobalt-lantern-77".to_owned();
    // Two copies at once. The page clears the first one's copy event, and
    // fills the second one's with too much text. Servo reports both keys
    // handled after both.
    let first = press(&page, "ctrl-c", now);
    let second = press(&page, "ctrl-c", now);
    page.write(Write::Clear, now);
    page.write(Write::Clear, now);
    page.write(Write::Text("x".repeat(TEXT + 1)), now);
    page.handled(first, now);
    page.handled(second, now);
    assert_eq!(
        pasted(&mailbox, &page, &mut system, now),
        Some(Some(String::new()))
    );
}

#[test]
fn a_copy_with_one_oversized_text_among_its_texts_changes_nothing() {
    let orders = [
        ["plum-harbor-12".to_owned(), "x".repeat(TEXT + 1)],
        ["x".repeat(TEXT + 1), "plum-harbor-12".to_owned()],
    ];
    let now = Instant::now();
    for texts in orders {
        let (mailbox, page) = focused_page();
        let mut system = "cobalt-lantern-77".to_owned();
        // The page fills one copy event with two texts: Servo clears, then
        // sends each. The window takes its turn whenever it is woken.
        let copy = press(&page, "ctrl-c", now);
        page.write(Write::Clear, now);
        for text in texts {
            page.write(Write::Text(text), now);
            serve(&mailbox, &page, &mut system, now);
        }
        page.handled(copy, now);
        assert_eq!(
            pasted(&mailbox, &page, &mut system, now),
            Some(Some("cobalt-lantern-77".to_owned()))
        );
    }
}

#[test]
fn a_cut_the_page_fills_reaches_the_paste_right_after_it() {
    let (mailbox, page) = focused_page();
    let now = Instant::now();
    let mut system = "cobalt-lantern-77".to_owned();
    // Ctrl+X, then Ctrl+V at once: Servo sends the cut's clear and text,
    // then waits for the paste's text, and reports no key handled before.
    let cut = press(&page, "ctrl-x", now);
    page.write(Write::Clear, now);
    page.write(Write::Text("tundra-violin-36".to_owned()), now);
    assert_eq!(
        pasted(&mailbox, &page, &mut system, now),
        Some(Some("tundra-violin-36".to_owned()))
    );
    page.handled(cut, now);
}

#[test]
fn a_copy_the_page_fills_changes_nothing_after_its_grant_ended() {
    // The grant ends through the window alone, which stopped being the
    // active one, or at its deadline.
    for at_deadline in [false, true] {
        let (mailbox, page) = focused_page();
        let now = Instant::now();
        let mut system = "cobalt-lantern-77".to_owned();
        let copy = press(&page, "ctrl-c", now);
        page.write(Write::Clear, now);
        page.write(Write::Text("saffron-ferry-24".to_owned()), now);
        let later = if at_deadline {
            now + GESTURE
        } else {
            mailbox.clipboard.revoke();
            now
        };
        // Servo's report of the key, and no other copy key.
        page.handled(copy, later);
        serve(&mailbox, &page, &mut system, later);
        assert_eq!(system, "cobalt-lantern-77");
    }
}

#[test]
fn a_copy_left_by_an_ended_grant_takes_no_later_copy_with_it() {
    for at_deadline in [false, true] {
        let (mailbox, page) = focused_page();
        let now = Instant::now();
        let mut system = "cobalt-lantern-77".to_owned();
        // The page clears the first key's copy event, and Servo does not
        // report the key handled before its grant ends.
        let first = press(&page, "ctrl-c", now);
        page.write(Write::Clear, now);
        let later = if at_deadline {
            now + GESTURE
        } else {
            mailbox.clipboard.revoke();
            now + Duration::from_secs(1)
        };
        // The next key copies a selection.
        let second = press(&page, "ctrl-c", later);
        page.write(Write::Text("juniper-atlas-53".to_owned()), later);
        assert_eq!(
            pasted(&mailbox, &page, &mut system, later),
            Some(Some("juniper-atlas-53".to_owned()))
        );
        // Servo's late reports change nothing more.
        page.handled(first, later);
        page.handled(second, later);
        serve(&mailbox, &page, &mut system, later);
        assert_eq!(system, "juniper-atlas-53");
    }
}
