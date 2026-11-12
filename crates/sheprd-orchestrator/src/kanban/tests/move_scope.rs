#![allow(clippy::unwrap_used)]

use super::*;
use moves::{
    Attempt, ConfirmedMove, Context, Fresh, Label, LabelId, Labels, MoveGate, MovePlan, Proposal,
};
use std::sync::Arc;

pub(crate) fn fixture() -> (Proposal, Fresh) {
    let source = source(Provider::Forgejo, "forgejo.example.test", "team/app");
    let item = issue(&source, "7", "open", &[]);
    let card = CardKey::try_from(&item.key).unwrap();
    let mapping = crate::kanban::config::LabelMap::default();
    let fresh = Fresh {
        context: Context {
            account: "dummy-account".into(),
            account_generation: 1,
            api: "https://forgejo.example.test/".parse().unwrap(),
            board_generation: 1,
            mapping: mapping.clone(),
            mapping_generation: 1,
            route: Route {
                origin: Origin::Checkout(source.clone()),
                worker: uuid::Uuid::new_v4(),
            },
            source_generation: uuid::Uuid::new_v4(),
        },
        facts: complete(),
        item,
        labels: Labels {
            in_progress: Some(Label {
                exclusive: false,
                id: LabelId::Forgejo(10),
                name: mapping.in_progress_label().into(),
            }),
            in_review: Some(Label {
                exclusive: false,
                id: LabelId::Forgejo(20),
                name: mapping.in_review_label().into(),
            }),
        },
    };
    let proposal = Proposal {
        board_generation: 1,
        card: card.clone(),
        destination: BoardColumn::InProgress,
        destination_lane: card.lane,
    };
    (proposal, fresh)
}

#[test]
fn approvals_refuse_missing_cancelled_reused_stale_and_cross_scope_without_mutation() {
    let (proposal, mut fresh) = fixture();
    fresh.item.labels = vec!["unrelated".into(), "another".into()];
    let plan = Arc::new(
        MovePlan::prepare(proposal.clone(), fresh.clone())
            .unwrap()
            .unwrap(),
    );
    let gate = Arc::new(MoveGate::new(4));
    let mut reordered = fresh.clone();
    reordered.item.labels.reverse();
    reordered.item.labels.push("unrelated".into());
    assert!(plan.revalidate(&reordered).is_ok());
    let mut mutations = Vec::new();
    for change in 0..10 {
        let mut changed = fresh.clone();
        match change {
            0 => changed.context.account = "another-account".into(),
            1 => changed.context.source_generation = uuid::Uuid::new_v4(),
            2 => changed.context.api = "https://github.example.test/".parse().unwrap(),
            3 => {
                changed.context.route.origin = Origin::Checkout(source(
                    Provider::Forgejo,
                    "forgejo.example.test",
                    "team/another",
                ))
            }
            4 => changed.item.state = "closed".into(),
            5 => changed.labels.in_progress.as_mut().unwrap().id = LabelId::Forgejo(99),
            6 => changed.context.board_generation += 1,
            7 => changed
                .item
                .labels
                .push(changed.context.mapping.in_review_label().into()),
            8 => changed.context.mapping_generation += 1,
            _ => changed.item.labels = vec!["different".into(), "another".into()],
        }
        let approval = ConfirmedMove::from_dialog(plan.clone(), gate.acquire(plan.card()).unwrap());
        let result = approval.perform_for_test(
            || Ok(changed.clone()),
            |_, change| {
                mutations.push(change.clone());
                Attempt::Applied
            },
        );
        assert!(matches!(result.outcome, moves::Outcome::Refused(_)));
        assert!(!result.attempted);
        assert!(mutations.is_empty());
    }
    let mut cross_lane = proposal.clone();
    cross_lane.destination_lane = LaneKey::try_from(&source(
        Provider::Forgejo,
        "forgejo.example.test",
        "team/other",
    ))
    .unwrap();
    assert!(matches!(
        MovePlan::prepare(cross_lane, fresh.clone()),
        Err(Error::CrossLane)
    ));
    let mut cross_host = proposal.clone();
    cross_host.card.item.source.host = "github.example.test".into();
    assert!(matches!(
        MovePlan::prepare(cross_host, fresh.clone()),
        Err(Error::Route)
    ));
    for state in ["unknown", "draft"] {
        let mut invalid = fresh.clone();
        invalid.item.state = state.into();
        assert!(matches!(
            MovePlan::prepare(proposal.clone(), invalid),
            Err(Error::ReadOnlyMove)
        ));
    }
    let mut pr = fresh.clone();
    pr.item.key.native_id = "pr/7".into();
    let mut pr_proposal = proposal.clone();
    pr_proposal.card = CardKey::try_from(&pr.item.key).unwrap();
    assert!(matches!(
        MovePlan::prepare(pr_proposal, pr),
        Err(Error::ReadOnlyMove)
    ));
    let mut incomplete = fresh.clone();
    incomplete.facts.labels = Some(LabelCompleteness::Incomplete);
    assert!(matches!(
        MovePlan::prepare(proposal.clone(), incomplete),
        Err(Error::ReadOnlyMove)
    ));
    let mut missing = fresh.clone();
    missing.labels.in_progress = None;
    assert!(matches!(
        MovePlan::prepare(proposal.clone(), missing),
        Err(Error::MissingLabel)
    ));
    let mut exclusive = fresh.clone();
    exclusive.labels.in_progress.as_mut().unwrap().exclusive = true;
    assert!(matches!(
        MovePlan::prepare(proposal.clone(), exclusive),
        Err(Error::UnsafeLabel)
    ));
    let mut anonymous = fresh.clone();
    anonymous.context.account.clear();
    assert!(matches!(
        MovePlan::prepare(proposal, anonymous),
        Err(Error::MovePermission)
    ));
}
