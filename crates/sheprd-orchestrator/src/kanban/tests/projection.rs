use super::*;
use crate::kanban::facts::{MappingWarning, ReadOnlyReason};

#[test]
fn conflicting_unknown_incomplete_and_closed_states_never_authorize_a_move() -> Result<()> {
    let mappings = Mappings::default();
    let progress = "status: in-progress";
    let review = "status: in-review";
    for provider in [Provider::Github, Provider::Forgejo] {
        let host = match provider {
            Provider::Github => "github.example.test",
            _ => "forgejo.example.test",
        };
        let source = source(provider, host, "team/app");
        let route = checkout(&source, 1);
        let incomplete = WorkflowFacts {
            labels: Some(LabelCompleteness::Incomplete),
            ..complete()
        };
        let cases = [
            (
                "1",
                "open",
                vec![],
                complete(),
                BoardColumn::Open,
                None,
                MoveCapability::PreflightRequired,
            ),
            (
                "2",
                "open",
                vec![progress],
                complete(),
                BoardColumn::InProgress,
                None,
                MoveCapability::PreflightRequired,
            ),
            (
                "3",
                "open",
                vec![review],
                complete(),
                BoardColumn::InReview,
                None,
                MoveCapability::PreflightRequired,
            ),
            (
                "4",
                "open",
                vec![progress, review],
                complete(),
                BoardColumn::Open,
                Some(MappingWarning::ConflictingLabels),
                MoveCapability::PreflightRequired,
            ),
            (
                "5",
                "unknown",
                vec![progress],
                complete(),
                BoardColumn::Open,
                Some(MappingWarning::UnknownNativeState),
                MoveCapability::ReadOnly(ReadOnlyReason::UnknownState),
            ),
            (
                "6",
                "open",
                vec![progress],
                incomplete.clone(),
                BoardColumn::Open,
                Some(MappingWarning::IncompleteLabels),
                MoveCapability::ReadOnly(ReadOnlyReason::IncompleteEvidence),
            ),
            (
                "7",
                "closed",
                vec![progress, review],
                complete(),
                BoardColumn::Done,
                Some(MappingWarning::ConflictingLabels),
                MoveCapability::PreflightRequired,
            ),
            (
                "8",
                "closed",
                vec![review],
                incomplete,
                BoardColumn::Done,
                Some(MappingWarning::IncompleteLabels),
                MoveCapability::ReadOnly(ReadOnlyReason::IncompleteEvidence),
            ),
            (
                "9",
                "open",
                vec![],
                WorkflowFacts::default(),
                BoardColumn::Open,
                Some(MappingWarning::UnknownProvenance),
                MoveCapability::ReadOnly(ReadOnlyReason::IncompleteEvidence),
            ),
            (
                "10",
                "unlisted",
                vec![],
                WorkflowFacts::default(),
                BoardColumn::Open,
                Some(MappingWarning::UnknownNativeState),
                MoveCapability::ReadOnly(ReadOnlyReason::UnknownState),
            ),
            (
                "pr/11",
                "draft",
                vec![],
                complete(),
                BoardColumn::InProgress,
                None,
                MoveCapability::ReadOnly(ReadOnlyReason::PullRequest),
            ),
            (
                "pr/12",
                "open",
                vec![progress],
                complete(),
                BoardColumn::InReview,
                None,
                MoveCapability::ReadOnly(ReadOnlyReason::PullRequest),
            ),
            (
                "pr/13",
                "merged",
                vec![],
                complete(),
                BoardColumn::Done,
                None,
                MoveCapability::ReadOnly(ReadOnlyReason::PullRequest),
            ),
            (
                "pr/14",
                "closed",
                vec![],
                complete(),
                BoardColumn::Done,
                None,
                MoveCapability::ReadOnly(ReadOnlyReason::PullRequest),
            ),
            (
                "pr/15",
                "unknown",
                vec![],
                complete(),
                BoardColumn::Open,
                Some(MappingWarning::UnknownNativeState),
                MoveCapability::ReadOnly(ReadOnlyReason::PullRequest),
            ),
        ];
        for (id, state, labels, facts, column, warning, capability) in cases {
            let mut item = issue(&source, id, state, &labels);
            item.assignee = Some("someone".into());
            item.blocked_by.push("unrelated".into());
            let board = project(
                [],
                [CardInput {
                    facts: &facts,
                    item: &item,
                    route: &route,
                }],
                &mappings,
                |_, _| true,
            )?;
            let Some(card) = cards(&board).next() else {
                panic!("missing fixture card");
            };
            assert_eq!(card.placement.column, column);
            assert_eq!(card.placement.move_capability, capability);
            if let Some(warning) = warning {
                assert!(card.placement.warnings.contains(&warning));
            }
            assert_eq!(card.item, item);
            assert_eq!(card.facts, facts);
        }
    }
    Ok(())
}
