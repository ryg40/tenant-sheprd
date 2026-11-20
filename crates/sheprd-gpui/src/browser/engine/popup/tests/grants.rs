//! Which input of the user lets a page open a new window, and until when.
use super::*;
use crate::browser::engine::clipboard::{self, Clipboard};
use servo::MouseMoveEvent;

#[test]
fn a_new_window_with_no_input_of_the_user_before_it_opens_no_tab() {
    let (gate, waiting, now) = (Activation::default(), Waiting::default(), Instant::now());
    // The pointer passes over the page, which presses nothing.
    let moved = InputEvent::MouseMove(MouseMoveEvent::new(point()));
    send(&gate, &waiting, &moved, now);
    assert!(!gate.admit(&waiting, now));
}

#[test]
fn two_new_windows_after_one_click_open_one_tab() {
    let (gate, waiting, now) = (Activation::default(), Waiting::default(), Instant::now());
    for action in [MouseButtonAction::Down, MouseButtonAction::Up] {
        send(&gate, &waiting, &button(action), now);
    }
    let opened = [gate.admit(&waiting, now), gate.admit(&waiting, now)];
    assert_eq!(opened, [true, false]);
}

#[test]
fn a_release_grants_nothing_and_renews_no_grant() {
    let (gate, waiting, start) = (Activation::default(), Waiting::default(), Instant::now());
    // A release whose press the page did not get.
    send(&gate, &waiting, &button(MouseButtonAction::Up), start);
    assert!(!gate.admit(&waiting, start));
    // A press, and its release late in the grant's time.
    send(&gate, &waiting, &button(MouseButtonAction::Down), start);
    let late = start + GRANT - Duration::from_millis(100);
    send(&gate, &waiting, &button(MouseButtonAction::Up), late);
    assert!(!gate.admit(&waiting, start + GRANT + Duration::from_millis(100)));
}

#[test]
fn a_grant_older_than_its_bound_opens_no_tab() {
    let (gate, waiting, pressed) = (Activation::default(), Waiting::default(), Instant::now());
    send(&gate, &waiting, &button(MouseButtonAction::Down), pressed);
    assert!(!gate.admit(&waiting, pressed + GRANT + Duration::from_millis(1)));
}

#[test]
fn a_load_a_reload_or_an_address_change_of_the_page_revokes_its_grant() {
    let changes: &[fn(&Clipboard<()>)] = &[
        |page| {
            let load = Command::Load(0, "https://example.com/".to_owned());
            clipboard::before_command(&load, |_| Some(page));
        },
        |page| clipboard::before_command(&Command::Reload(0), |_| Some(page)),
        |page| clipboard::before_command(&Command::History(0, false), |_| Some(page)),
        // What the page's delegate does as Servo reports another address.
        Clipboard::forget,
    ];
    for change in changes {
        let mailbox = Arc::new(Mailbox::default());
        let page = Clipboard::<()>::new(mailbox.clone(), Arc::default());
        let (gate, now) = (Activation::default(), Instant::now());
        send(
            &gate,
            &mailbox.clipboard,
            &button(MouseButtonAction::Down),
            now,
        );
        change(&page);
        assert!(!gate.admit(&mailbox.clipboard, now));
    }
}
