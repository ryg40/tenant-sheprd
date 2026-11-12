#![allow(clippy::unwrap_used)]

use super::*;
use moves::{Attempt, ConfirmedMove, MoveGate, MovePlan, Mutation, Outcome};
use std::{cell::RefCell, collections::HashSet, sync::Arc};

#[test]
fn failed_partial_and_lost_responses_keep_observed_state_without_replay() {
    for mode in [
        "refused",
        "partial",
        "lost",
        "timeout",
        "read-failed",
        "verified",
    ] {
        let (proposal, mut fresh) = move_scope::fixture();
        fresh.item.state = "closed".into();
        fresh.item.labels = vec![
            fresh.context.mapping.in_review_label().into(),
            "unrelated".into(),
        ];
        let plan = Arc::new(MovePlan::prepare(proposal, fresh.clone()).unwrap().unwrap());
        let original = fresh.item.clone();
        let tracker = RefCell::new(fresh);
        let gate = Arc::new(MoveGate::new(4));
        let approval = ConfirmedMove::from_dialog(plan.clone(), gate.acquire(plan.card()).unwrap());
        let mut mutations = HashSet::new();
        let mut read_before = true;
        let completion = approval.perform_for_test(
            || {
                if !read_before && matches!(mode, "timeout" | "read-failed") {
                    return Err(crate::Error::Worker(std::io::Error::from(
                        std::io::ErrorKind::TimedOut,
                    )));
                }
                read_before = false;
                Ok(tracker.borrow().clone())
            },
            |_, mutation| {
                // Duplicate transport operations fail the fake peer rather than being counted.
                assert!(
                    mutations.insert(format!("{mutation:?}")),
                    "replayed mutation"
                );
                let error = Arc::new(crate::Error::Worker(std::io::Error::from(
                    std::io::ErrorKind::TimedOut,
                )));
                if mode == "refused" {
                    return Attempt::Refused(error);
                }
                if mode == "partial" && matches!(mutation, Mutation::AddLabel(_)) {
                    return Attempt::Refused(error);
                }
                if mode == "timeout" {
                    return Attempt::Unknown(error);
                }
                let mut tracker = tracker.borrow_mut();
                match mutation {
                    Mutation::RemoveLabel(_) => tracker
                        .item
                        .labels
                        .retain(|name| name != plan.context().mapping.in_review_label()),
                    Mutation::AddLabel(_) => tracker
                        .item
                        .labels
                        .push(plan.context().mapping.in_progress_label().into()),
                    Mutation::SetState(state) => tracker.item.state = state.as_str().into(),
                }
                if mode == "lost" {
                    Attempt::Unknown(error)
                } else {
                    Attempt::Applied
                }
            },
        );
        match mode {
            "refused" => assert!(matches!(completion.outcome, Outcome::Refused(_))),
            "partial" => assert!(matches!(completion.outcome, Outcome::Partial { .. })),
            "lost" | "timeout" | "read-failed" => {
                assert!(matches!(completion.outcome, Outcome::Unknown { .. }))
            }
            _ => assert!(matches!(completion.outcome, Outcome::Verified)),
        }
        if let Some(observed) = completion.observation {
            assert_eq!(observed.item, tracker.borrow().item);
            assert!(
                observed
                    .item
                    .labels
                    .iter()
                    .any(|label| label == "unrelated")
            );
        } else {
            assert_eq!(plan.original().item, original);
            assert!(completion.read_error.is_some());
        }
        if matches!(mode, "refused" | "timeout") {
            assert_eq!(tracker.borrow().item, original);
        }
        assert_eq!(plan.original().item, original);
    }
}
