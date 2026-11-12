use crate::walkr::{
    Connectivity,
    run::{self, Pane, Run},
    sessions::live_session,
};
use sheprd_control_protocol::protocol::walkr::RunAddress;

const SESSION: &str = "20261101T120000Z-0123456789ab";

fn run(fragment: &str) -> Run {
    let address = RunAddress::new(format!(
        "http://127.0.0.1:4100/_walkr/compare/g/INVENTEDSECRET#{fragment}"
    ));
    Run::new(run::parse(&address).unwrap(), Pane::new("p1", None), 1)
}

#[test]
fn a_row_is_live_only_under_all_three_conditions() {
    let live = |run: Option<&Run>, state| live_session(run, state) == Some(SESSION);
    let held = run(SESSION);
    let mut ended = run(SESSION);
    assert!(ended.leave(&Pane::new("p1", None)));
    let other = run("20261101T120000Z-ffffffffffff");
    // All three: a pane holds the run, its probe answered, its fragment.
    assert!(live(Some(&held), Connectivity::Reachable));
    assert!(live(Some(&held), Connectivity::Ready));
    // Only the fragment and the answer: no pane holds the run.
    assert!(!live(Some(&ended), Connectivity::Reachable));
    // Only the pane and the fragment: the probe has not answered, or failed.
    assert!(!live(Some(&held), Connectivity::Checking));
    assert!(!live(
        Some(&held),
        Connectivity::Unreachable {
            cause: crate::walkr::Unreachable::Connect
        }
    ));
    // Only the pane and the answer: the address names another session.
    assert!(!live(Some(&other), Connectivity::Reachable));
    // No run at all.
    assert!(!live(None, Connectivity::Reachable));
}
