use super::*;
use crate::{
    actions::{Action, DispatchRequest},
    kanban::{self, moves::Outcome},
    tests::forgejo_kanban_moves::{Peer, fixture, session},
};

#[test]
fn account_moves_reach_only_the_verified_repository_and_checkout_actions_stay_refused() {
    let dir = tempfile::tempdir().unwrap();
    let worker = worker(dir.path());
    let (proposal, context) = fixture("", true);
    let peer = Arc::new(Mutex::new(Peer::default()));
    let session = session(&proposal, context.clone(), peer.clone());
    let (session, fresh) = session.connect().unwrap();
    let observed = kanban::history::Record {
        item: fresh.item.clone(),
        facts: fresh.facts.clone(),
    };
    let ticket = worker
        .moves
        .prepare_forgejo(proposal.clone(), session.clone(), observed)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if worker.moves.snapshot().iter().any(|status| {
            matches!(
                status.phase,
                super::super::super::MovePhase::AwaitingConfirmation(_)
            )
        }) {
            break;
        }
        assert!(Instant::now() < deadline, "preflight did not finish");
        std::thread::yield_now();
    }
    let approval = worker.moves.confirm(ticket).unwrap();
    // Preparation and approval alone send no mutation.
    assert!(peer.lock().unwrap().mutations.is_empty());
    worker.moves.submit_forgejo(approval, session).unwrap();
    let completion = loop {
        if let Some(super::super::super::MoveStatus {
            phase: super::super::super::MovePhase::Finished(completion),
            ..
        }) = worker
            .moves
            .snapshot()
            .into_iter()
            .find(|s| s.card == proposal.card)
        {
            break completion;
        }
        assert!(Instant::now() < deadline, "move did not finish");
        std::thread::yield_now();
    };
    assert!(matches!(completion.outcome, Outcome::Verified));
    let held = peer.lock().unwrap();
    assert!(
        held.mutations
            .iter()
            .all(|(_, path, _)| path.starts_with("/api/v1/repos/team/app/issues/7"))
    );
    assert_eq!(
        held.labels
            .iter()
            .map(|v| v["name"].as_str().unwrap())
            .collect::<std::collections::HashSet<_>>(),
        std::collections::HashSet::from(["unrelated", "status: in-progress"])
    );
    drop(held);

    let peer = Arc::new(Mutex::new(Peer {
        account_repositories: vec!["team/another".into()],
        ..Peer::default()
    }));
    let result =
        crate::tests::forgejo_kanban_moves::session(&proposal, context.clone(), peer.clone())
            .connect();
    assert!(matches!(result, Err(Error::Kanban(kanban::Error::Route))));
    assert!(peer.lock().unwrap().mutations.is_empty());

    let peer = Arc::new(Mutex::new(Peer {
        issue_route: Some(format!("https://{HOST}/api/v1/repos/team/another/issues/7")),
        ..Peer::default()
    }));
    let result =
        crate::tests::forgejo_kanban_moves::session(&proposal, context, peer.clone()).connect();
    assert!(matches!(result, Err(Error::Kanban(kanban::Error::Route))));
    assert!(peer.lock().unwrap().mutations.is_empty());

    for action in [
        Action::Dispatch(Box::new(DispatchRequest {
            item_key: proposal.card.item.canonical(),
            kind: sheprd_remote::teleport::AgentKind::Claude,
            model: None,
            prompt: "test".into(),
            title: "test".into(),
            branch: "test".into(),
            workspace_id: "unused".into(),
            base: None,
            elsewhere: None,
            role: None,
        })),
        Action::Stop {
            run: "unused".into(),
        },
        Action::Send {
            run: "unused".into(),
            text: "unused".into(),
        },
        Action::Remove {
            run: "unused".into(),
        },
        Action::DeleteBead {
            source: proposal.card.item.source.clone(),
            id: "unused".into(),
        },
    ] {
        worker.act(action);
        let notice = worker.notices.lock().unwrap().pop_front().unwrap();
        assert!(
            matches!(notice.outcome, Err(ref error) if matches!(**error, Error::Forgejo(forgejo::Error::NoCheckout { .. })))
        );
    }
    assert!(worker.snapshot.access.is_none());
    assert!(worker.store.as_ref().unwrap().items().unwrap().is_empty());
}
