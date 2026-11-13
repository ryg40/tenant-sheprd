use super::*;
use crate::kanban::{
    self,
    history::Record,
    identity::CardKey,
    moves::{Attempt, MoveGate, Mutation},
};

fn settled(moves: &Moves, card: &CardKey, predicate: impl Fn(&MovePhase) -> bool) -> MoveStatus {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = moves
            .snapshot()
            .into_iter()
            .find(|status| &status.card == card)
            && predicate(&status.phase)
        {
            return status;
        }
        assert!(Instant::now() < until, "move did not settle");
        std::thread::yield_now();
    }
}

#[test]
fn missing_cancelled_and_reused_approval_never_writes_across_windows() {
    let (proposal, fresh) = kanban::tests::move_scope::fixture();
    let gate = Arc::new(MoveGate::new(MAX_ACTIONS));
    let first = Moves::new(gate.clone(), Arc::default());
    let second = Moves::new(gate, Arc::default());
    for moves in [&first, &second] {
        moves
            .configure(proposal.card.lane.clone(), fresh.context.clone())
            .unwrap();
    }
    let (send, recv) = mpsc::sync_channel(1);
    let (started, starting) = mpsc::sync_channel(1);
    let pending = fresh.clone();
    let ticket = first
        .prepare(
            proposal.clone(),
            Record {
                item: fresh.item.clone(),
                facts: fresh.facts.clone(),
            },
            move |_| {
                started.send(()).unwrap();
                recv.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(pending)
            },
        )
        .unwrap();
    assert!(matches!(
        second.prepare(
            proposal.clone(),
            Record {
                item: fresh.item.clone(),
                facts: fresh.facts.clone()
            },
            |_| panic!("busy read")
        ),
        Err(Error::Kanban(kanban::Error::MoveBusy))
    ));
    starting.recv_timeout(Duration::from_secs(5)).unwrap();
    first.cancel(&proposal.card);
    send.send(()).unwrap();
    assert!(matches!(
        first.confirm(ticket),
        Err(Error::Kanban(kanban::Error::MissingApproval))
    ));
    let pending = fresh.clone();
    let ticket = second
        .prepare(
            proposal.clone(),
            Record {
                item: fresh.item.clone(),
                facts: fresh.facts.clone(),
            },
            move |_| Ok(pending),
        )
        .unwrap();
    settled(&second, &proposal.card, |phase| {
        matches!(phase, MovePhase::AwaitingConfirmation(_))
    });
    let approval = second.confirm(ticket).unwrap();
    second.cancel(&proposal.card);
    assert!(matches!(
        second.submit(
            approval,
            || panic!("cancelled read"),
            |_, _| panic!("cancelled mutation")
        ),
        Err(Error::Kanban(kanban::Error::MissingApproval))
    ));
}

#[test]
fn old_sync_and_result_cannot_overwrite_a_new_source_or_same_number_card() {
    let (proposal, fresh) = kanban::tests::move_scope::fixture();
    let actions = Arc::new(AtomicUsize::new(0));
    let moves = Moves::new(Arc::new(MoveGate::new(MAX_ACTIONS)), actions.clone());
    moves
        .configure(proposal.card.lane.clone(), fresh.context.clone())
        .unwrap();
    let old = moves.begin_read(&proposal.card.lane).unwrap();
    let original = Record {
        item: fresh.item.clone(),
        facts: fresh.facts.clone(),
    };
    let pending = fresh.clone();
    let ticket = moves
        .prepare(proposal.clone(), original.clone(), move |_| Ok(pending))
        .unwrap();
    settled(&moves, &proposal.card, |phase| {
        matches!(phase, MovePhase::AwaitingConfirmation(_))
    });
    let approval = moves.confirm(ticket).unwrap();
    let tracker = Arc::new(Mutex::new(fresh.clone()));
    let read = tracker.clone();
    let write = tracker.clone();
    moves
        .submit(
            approval,
            move || Ok(read.lock().unwrap().clone()),
            move |plan, mutation| {
                if matches!(mutation, Mutation::AddLabel(_)) {
                    write
                        .lock()
                        .unwrap()
                        .item
                        .labels
                        .push(plan.context().mapping.in_progress_label().into());
                }
                Attempt::Applied
            },
        )
        .unwrap();
    let completed = settled(&moves, &proposal.card, |phase| {
        matches!(phase, MovePhase::Finished(_))
    });
    assert!(!moves.observe(&old, std::slice::from_ref(&original)));
    assert!(moves.take_invalidated().contains(&proposal.card.lane));
    let snapshot = Snapshot {
        items: Arc::new(vec![original.item.clone()]),
        moves: Arc::new(moves.snapshot()),
        ..Snapshot::default()
    };
    assert!(
        snapshot
            .board_records(Utc::now())
            .iter()
            .any(|record| record.item == completed.observed.item)
    );
    assert_ne!(completed.observed.item, original.item);
    let fence = moves.begin_read(&proposal.card.lane).unwrap();
    let mut wrong = original.clone();
    wrong.item.key.source.host = "github.example.test".into();
    assert!(moves.observe(&fence, &[wrong]));
    assert_eq!(moves.snapshot()[0].observed.item, completed.observed.item);

    // An accepted write may finish after replacement, but its result stays fenced.
    let mut proposal_back = proposal.clone();
    proposal_back.destination = kanban::BoardColumn::Open;
    let pending = tracker.lock().unwrap().clone();
    let ticket = moves
        .prepare(proposal_back, completed.observed.clone(), move |_| {
            Ok(pending)
        })
        .unwrap();
    settled(&moves, &proposal.card, |phase| {
        matches!(phase, MovePhase::AwaitingConfirmation(_))
    });
    let approval = moves.confirm(ticket).unwrap();
    let (started, starting) = mpsc::sync_channel(1);
    let (resume, resumed) = mpsc::sync_channel(1);
    let read = tracker.clone();
    moves
        .submit(
            approval,
            move || Ok(read.lock().unwrap().clone()),
            move |_, _| {
                started.send(()).unwrap();
                resumed.recv_timeout(Duration::from_secs(5)).unwrap();
                Attempt::Applied
            },
        )
        .unwrap();
    starting.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut replacement = fresh.context.clone();
    replacement.source_generation = uuid::Uuid::new_v4();
    moves
        .configure(proposal.card.lane.clone(), replacement)
        .unwrap();
    resume.send(()).unwrap();
    assert!(!moves.observe(&fence, &[original]));
    let until = Instant::now() + Duration::from_secs(5);
    while actions.load(Ordering::SeqCst) != 0 {
        assert!(Instant::now() < until, "accepted action did not finish");
        std::thread::yield_now();
    }
    assert!(moves.snapshot().is_empty());
}
