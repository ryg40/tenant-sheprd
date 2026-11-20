//! A stop acts only on a process proven to be the one listed, against a
//! scripted OS: no real process is looked up or signalled.

use super::*;
use crate::local_ports::process::{Found, Tie, act_with};
use std::cell::RefCell;
use sysinfo::Signal;

const USER: u32 = 501;
const LISTED: Victim = Victim {
    identity: Identity {
        pid: 4242,
        started: 1_700_000_000,
    },
    birth: Some(Birth(8_675_309)),
};

/// What the pid reads when it still names the process listed.
const SAME: Found = Found {
    birth: LISTED.birth,
    tie: Tie::Held,
    user: Some(USER),
    zombie: false,
};

/// The outcome of stopping `victim` while its pid reads `found`, and every
/// signal sent.
fn stop(victim: Victim, found: Found) -> (Outcome, Vec<Signal>) {
    let sent = RefCell::new(Vec::new());
    let outcomes = act_with(
        PortAction::Stop,
        &[victim],
        USER,
        Duration::ZERO,
        |_| Some(found),
        |_, signal| {
            sent.borrow_mut().push(signal);
            true
        },
        |_| {},
    );
    (outcomes[0].1, sent.into_inner())
}

#[test]
fn a_pid_that_now_names_another_process_gets_no_signal() {
    // The same second as the listed process, which whole seconds could
    // not tell apart.
    let reused = Found {
        birth: Some(Birth(8_675_310)),
        ..SAME
    };
    assert_eq!(stop(LISTED, reused), (Outcome::Exited, vec![]));
    // The fixture does stop the process listed.
    assert_eq!(stop(LISTED, SAME).1.first(), Some(&Signal::Term));
}

#[test]
fn a_process_without_proof_gets_no_signal() {
    let unowned = Found { user: None, ..SAME };
    let unread = Found {
        birth: None,
        ..SAME
    };
    let unheld = Found {
        tie: Tie::Missing,
        ..SAME
    };
    for found in [unowned, unread, unheld] {
        assert_eq!(stop(LISTED, found), (Outcome::Unidentified, vec![]));
    }
    // A row whose own start could not be read proves nothing either.
    let unlisted = Victim {
        birth: None,
        ..LISTED
    };
    assert_eq!(stop(unlisted, SAME), (Outcome::Unidentified, vec![]));
}
