//! Clear takes the waiting sessions out of the count, the waiting list and
//! the click of Walkr's icon, in Sheprd only. A session that waits later
//! counts again. Only two complete reads in a row of a host that do not
//! list a dismissed session forget it.
use crate::{
    automations::Machine,
    walkr::{
        history::pending::{self, Pending, Tally},
        waiting::{
            Dismissed, Error, Waiting,
            target::{Workspace, newest, place},
        },
    },
};
use chrono::{TimeZone as _, Utc};
use std::time::{Duration, Instant};

const FIRST: &str = "20261101T120000Z-0123456789ab";
const SECOND: &str = "20261102T120000Z-0123456789ab";
const LATER: &str = "20261103T120000Z-0123456789ab";

/// A read of a host whose sessions `ids` wait, oldest first.
fn read(ids: &[&str], complete: bool) -> Tally {
    let mut newest: Vec<Pending> = ids
        .iter()
        .zip(1..)
        .map(|(id, hour)| Pending {
            id: (*id).to_owned(),
            created: Utc.with_ymd_and_hms(2026, 11, 1, hour, 0, 0).single(),
            title: None,
            repo: Some("project".into()),
        })
        .collect();
    newest.reverse();
    let mut open: Vec<String> = ids.iter().map(|id| (*id).to_owned()).collect();
    open.sort();
    Tally {
        waiting: ids.len(),
        newest,
        sessions: ids.len(),
        live: false,
        open,
        complete,
    }
}

fn shown(waiting: &Waiting, index: usize) -> Vec<&str> {
    waiting.hosts()[index]
        .tally
        .as_ref()
        .unwrap()
        .newest
        .iter()
        .map(|pending| pending.id.as_str())
        .collect()
}

#[test]
fn a_cleared_walkr_session_leaves_the_count_and_a_new_one_counts() {
    let host = Machine::Ssh("host.example".into());
    let mut waiting = Waiting::default();
    waiting.test_answered(Machine::Local, "This computer", read(&[FIRST], true));
    waiting.test_answered(host.clone(), "host", read(&[FIRST, SECOND], true));
    assert_eq!(waiting.total(), 3);

    assert_eq!(waiting.clear(), 3);
    assert_eq!(waiting.total(), 0);
    assert!(shown(&waiting, 0).is_empty());
    assert!(shown(&waiting, 1).is_empty());
    assert!(newest(&waiting).is_none(), "no click target is left");
    assert!(waiting.take_dismissed_unsaved());
    // Nothing more to clear, and nothing to save.
    assert_eq!(waiting.clear(), 0);
    assert!(!waiting.take_dismissed_unsaved());

    // The same host has a new waiting session: it counts, and the icon
    // goes to it.
    waiting.test_read(1, Ok(read(&[FIRST, SECOND, LATER], true)));
    assert_eq!(waiting.total(), 1);
    assert_eq!(shown(&waiting, 1), [LATER]);
    let target = newest(&waiting).unwrap();
    assert_eq!((target.id, target.machine), (LATER, &host));
    assert_eq!(target.repo, Some("project"));

    // One row leaves on its own.
    waiting.dismiss(&host, LATER);
    assert_eq!(waiting.total(), 0);
    assert!(shown(&waiting, 1).is_empty());
}

#[test]
fn a_walkr_session_is_forgotten_only_when_two_complete_reads_miss_it() {
    let host = Machine::Ssh("host.example".into());
    let mut waiting = Waiting::default();
    waiting.test_answered(host.clone(), "host", read(&[FIRST, SECOND], true));
    assert_eq!(waiting.clear(), 2);

    // A failed read and a cut read forget nothing.
    waiting.test_read(0, Err(Error::TimedOut));
    waiting.test_read(0, Ok(read(&[SECOND], false)));
    assert_eq!(waiting.total(), 0);

    // One complete read misses FIRST, as when Walkr rewrites its record at
    // that moment: FIRST stays dismissed when it is listed again.
    waiting.test_read(0, Ok(read(&[SECOND], true)));
    waiting.test_read(0, Ok(read(&[FIRST, SECOND], true)));
    assert_eq!(waiting.total(), 0);

    // Two complete reads in a row miss FIRST: it is forgotten, and counts
    // again when it waits later.
    waiting.test_read(0, Ok(read(&[SECOND], true)));
    waiting.test_read(0, Ok(read(&[SECOND], true)));
    waiting.test_read(0, Ok(read(&[FIRST, SECOND], true)));
    assert_eq!(waiting.total(), 1);
    assert_eq!(shown(&waiting, 0), [FIRST]);
}

#[test]
fn a_walkr_dismissal_made_before_the_stored_set_loads_survives_it() {
    let mut waiting = Waiting::default();
    waiting.test_answered(Machine::Local, "This computer", read(&[FIRST], true));
    waiting.dismiss(&Machine::Local, FIRST);
    let mut stored = Dismissed::default();
    stored.insert(&Machine::Local, [SECOND]);
    waiting.restore_dismissed(stored);
    assert_eq!(waiting.total(), 0);
    assert!(shown(&waiting, 0).is_empty());
}

#[test]
fn the_next_walkr_page_keeps_its_workspace_when_the_newest_is_dismissed() {
    let (home, _elsewhere) = super::waiting_count::home();
    let read = pending::local(home.path(), Instant::now() + Duration::from_secs(5)).unwrap();
    let newer = read.newest[0].id.clone();
    let mut waiting = Waiting::default();
    waiting.test_answered(Machine::Local, "This computer", read);
    waiting.dismiss(&Machine::Local, &newer);
    let workspaces = [Workspace {
        active: false,
        endpoint: 0,
        holds_run: false,
        id: "older".into(),
        machine: Machine::Local,
        repo: Some("other".into()),
    }];
    let page = newest(&waiting).unwrap();
    assert_ne!(page.id, newer);
    let place = place(&page, None, &workspaces).map(|workspace| workspace.id.as_str());
    assert_eq!(place, Some("older"));
}
