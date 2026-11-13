//! Bounded move lifecycle, shared action admission, and retained observations.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Instant,
};
use uuid::Uuid;

use super::super::{MAX_ACTIONS, spawn_action};
use crate::{
    Result,
    kanban::{
        self,
        history::Record,
        identity::{CardKey, LaneKey},
        moves::{
            Attempt, Completion, ConfirmedMove, Context, Fresh, MoveGate, MovePermit, MovePlan,
            Mutation, Proposal,
        },
    },
};

/// All default repository workers use one app-wide gate, including different windows.
pub(in crate::service) fn shared_gate() -> Arc<MoveGate> {
    static GATE: OnceLock<Arc<MoveGate>> = OnceLock::new();
    GATE.get_or_init(|| Arc::new(MoveGate::new(MAX_ACTIONS)))
        .clone()
}

#[derive(Clone, Debug)]
pub enum MovePhase {
    Preparing,
    AwaitingConfirmation(Arc<MovePlan>),
    Writing,
    Finished(Arc<Completion>),
    Refused(Arc<crate::Error>),
    Cancelled,
    Unchanged,
}

#[derive(Clone, Debug)]
pub struct MoveStatus {
    pub card: CardKey,
    pub destination: kanban::BoardColumn,
    pub observed: Record,
    pub operation: Uuid,
    pub phase: MovePhase,
    pub source_generation: Uuid,
}

/// Preparation-issued authority. Dropping it cancels only its own unconfirmed operation.
#[must_use]
pub struct MoveTicket {
    armed: bool,
    card: CardKey,
    moves: Moves,
    operation: Uuid,
}

impl MoveTicket {
    pub fn operation(&self) -> Uuid {
        self.operation
    }
}

impl Drop for MoveTicket {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if let Ok(mut state) = self.moves.state.lock()
            && let Some(entry) = state.entries.get_mut(&self.card)
            && entry.status.operation == self.operation
        {
            cancel_entry(entry);
            self.moves.changed.store(true, Ordering::Release);
        }
    }
}

struct Entry {
    cancelled: Arc<AtomicBool>,
    permit: Option<MovePermit>,
    session: Option<crate::forgejo::moves::Session>,
    status: MoveStatus,
    terminal_at: Option<Instant>,
}

struct Source {
    context: Context,
    epoch: Uuid,
    invalidated: bool,
}

#[derive(Default)]
struct State {
    entries: HashMap<CardKey, Entry>,
    sources: HashMap<LaneKey, Source>,
}

/// A read started against an exact source incarnation and action epoch.
#[derive(Clone, Debug)]
pub struct ReadFence {
    epoch: Uuid,
    lane: LaneKey,
    source_generation: Uuid,
}

/// Clone this handle for action threads, never a service or provider transport.
#[derive(Clone)]
pub struct Moves {
    actions: Arc<AtomicUsize>,
    changed: Arc<AtomicBool>,
    gate: Arc<MoveGate>,
    generation: Uuid,
    state: Arc<Mutex<State>>,
}

impl Moves {
    pub(in crate::service) fn new(gate: Arc<MoveGate>, actions: Arc<AtomicUsize>) -> Self {
        Self {
            actions,
            changed: Arc::default(),
            gate,
            generation: Uuid::new_v4(),
            state: Arc::default(),
        }
    }

    pub fn generation(&self) -> Uuid {
        self.generation
    }

    pub(in crate::service) fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::AcqRel)
    }

    /// Replacing routing, account, board, or mapping invalidates unconfirmed plans.
    /// Accepted threads retain their original callbacks; late results are fenced.
    pub fn configure(&self, lane: LaneKey, context: Context) -> Result<()> {
        context.route.validate(&lane.source())?;
        let mut state = self.state.lock().map_err(|_| kanban::Error::MoveBusy)?;
        if state
            .sources
            .get(&lane)
            .is_some_and(|source| source.context == context)
        {
            return Ok(());
        }
        if !state.sources.contains_key(&lane)
            && state.sources.len() >= kanban::projection::MAX_LANES
        {
            return Err(kanban::Error::MoveBusy.into());
        }
        self.changed.store(true, Ordering::Release);
        state.entries.retain(|card, entry| {
            if card.lane != lane {
                return true;
            }
            entry.cancelled.store(true, Ordering::Release);
            false
        });
        state.sources.insert(
            lane,
            Source {
                context,
                epoch: Uuid::new_v4(),
                invalidated: false,
            },
        );
        Ok(())
    }

    pub(in crate::service) fn configure_dialog(
        &self,
        lane: LaneKey,
        context: Context,
    ) -> Result<()> {
        {
            let state = self.state.lock().map_err(|_| kanban::Error::MoveBusy)?;
            if let Some(source) = state.sources.get(&lane) {
                let mut provisional = source.context.clone();
                provisional.account.clear();
                provisional.account_generation = 0;
                if provisional == context {
                    return Ok(());
                }
            }
        }
        self.configure(lane, context)
    }

    pub fn clear(&self) {
        self.changed.store(true, Ordering::Release);
        if let Ok(mut state) = self.state.lock() {
            for entry in state.entries.values() {
                entry.cancelled.store(true, Ordering::Release);
            }
            state.entries.clear();
            state.sources.clear();
        }
    }

    /// Closing a view cancels preparation, not accepted mutations.
    pub fn cancel_unconfirmed(&self) {
        self.changed.store(true, Ordering::Release);
        if let Ok(mut state) = self.state.lock() {
            for entry in state.entries.values_mut() {
                cancel_entry(entry);
            }
        }
    }

    pub fn cancel(&self, card: &CardKey) {
        self.changed.store(true, Ordering::Release);
        if let Ok(mut state) = self.state.lock()
            && let Some(entry) = state.entries.get_mut(card)
        {
            cancel_entry(entry);
        }
    }

    pub fn snapshot(&self) -> Vec<MoveStatus> {
        let Ok(state) = self.state.lock() else {
            return Vec::new();
        };
        let mut statuses: Vec<_> = state
            .entries
            .values()
            .map(|entry| entry.status.clone())
            .collect();
        statuses.sort_by_key(|status| status.card.item.canonical());
        statuses
    }

    /// Runs a cancellable, read-only preflight within the existing action limit.
    pub fn prepare(
        &self,
        proposal: Proposal,
        observed: Record,
        read: impl FnOnce(&AtomicBool) -> Result<Fresh> + Send + 'static,
    ) -> Result<MoveTicket> {
        self.prepare_session(proposal, observed, move |cancelled| {
            read(cancelled).map(|fresh| (fresh, None))
        })
    }

    pub(in crate::service) fn prepare_session(
        &self,
        proposal: Proposal,
        observed: Record,
        read: impl FnOnce(&AtomicBool) -> Result<(Fresh, Option<crate::forgejo::moves::Session>)>
        + Send
        + 'static,
    ) -> Result<MoveTicket> {
        if proposal.card.lane != proposal.destination_lane {
            return Err(kanban::Error::CrossLane.into());
        }
        if CardKey::try_from(&observed.item.key)? != proposal.card {
            return Err(kanban::Error::Route.into());
        }
        let permit = self.gate.acquire(&proposal.card)?;
        let slot = ActionSlot::acquire(self.actions.clone())?;
        let mut state = self.state.lock().map_err(|_| kanban::Error::MoveBusy)?;
        let context = state
            .sources
            .get(&proposal.card.lane)
            .ok_or(kanban::Error::StaleMove)?
            .context
            .clone();
        if context.board_generation != proposal.board_generation {
            return Err(kanban::Error::StaleMove.into());
        }
        if !state.entries.contains_key(&proposal.card)
            && state.entries.len() >= kanban::projection::MAX_CARDS
        {
            // Retain recent outcomes across snapshots, but never retire an active callback.
            let oldest = state
                .entries
                .iter()
                .filter_map(|(card, entry)| entry.terminal_at.map(|at| (card, at)))
                .min_by_key(|(card, at)| (*at, card.item.canonical()))
                .map(|(card, _)| card.clone())
                .ok_or(kanban::Error::MoveBusy)?;
            state.entries.remove(&oldest);
        }
        let operation = Uuid::new_v4();
        let card = proposal.card.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.changed.store(true, Ordering::Release);
        state.entries.insert(
            card.clone(),
            Entry {
                cancelled: cancelled.clone(),
                permit: Some(permit),
                session: None,
                status: MoveStatus {
                    card: card.clone(),
                    destination: proposal.destination,
                    observed,
                    operation,
                    phase: MovePhase::Preparing,
                    source_generation: context.source_generation,
                },
                terminal_at: None,
            },
        );
        drop(state);
        let moves = self.clone();
        let prepared_card = card.clone();
        let spawned = spawn_action(move |_| {
            let _slot = slot;
            let result = if cancelled.load(Ordering::Acquire) {
                Err(crate::Error::Cancelled)
            } else {
                read(&cancelled).and_then(|(fresh, session)| {
                    let mut expected = context.clone();
                    if session.is_some() {
                        expected.account = fresh.context.account.clone();
                        expected.account_generation = fresh.context.account_generation;
                    }
                    if fresh.context != expected {
                        return Err(kanban::Error::StaleMove.into());
                    }
                    MovePlan::prepare(proposal, fresh)
                        .map(|plan| (plan, session))
                        .map_err(Into::into)
                })
            };
            moves.prepared(&prepared_card, operation, result);
        });
        if let Err(error) = spawned {
            self.prepared(&card, operation, Err(crate::Error::Worker(error)));
        }
        Ok(MoveTicket {
            armed: true,
            card,
            moves: self.clone(),
            operation,
        })
    }

    fn prepared(
        &self,
        card: &CardKey,
        operation: Uuid,
        result: Result<(Option<MovePlan>, Option<crate::forgejo::moves::Session>)>,
    ) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(entry) = state
            .entries
            .get_mut(card)
            .filter(|entry| entry.status.operation == operation)
        else {
            return;
        };
        if entry.cancelled.load(Ordering::Acquire) {
            entry.terminal_at = Some(Instant::now());
            return;
        }
        if let Ok((Some(plan), Some(_))) = &result
            && let Some(source) = state.sources.get_mut(&card.lane)
        {
            source.context = plan.context().clone();
        }
        let Some(entry) = state.entries.get_mut(card) else {
            return;
        };
        if entry.cancelled.load(Ordering::Acquire) {
            entry.terminal_at = Some(Instant::now());
            return;
        }
        self.changed.store(true, Ordering::Release);
        match result {
            Ok((Some(mut plan), session)) => {
                plan.set_operation(operation);
                entry.session = session;
                entry.status.observed = plan.original();
                entry.status.phase = MovePhase::AwaitingConfirmation(Arc::new(plan));
            }
            Ok((None, _)) => {
                entry.status.phase = MovePhase::Unchanged;
                entry.permit = None;
                entry.terminal_at = Some(Instant::now());
            }
            Err(error) => {
                entry.status.phase = MovePhase::Refused(Arc::new(error));
                entry.permit = None;
                entry.terminal_at = Some(Instant::now());
            }
        }
    }

    /// The native dialog consumes the preparation-issued ticket exactly once.
    pub fn confirm(&self, mut ticket: MoveTicket) -> Result<ConfirmedMove> {
        if !Arc::ptr_eq(&self.state, &ticket.moves.state) {
            return Err(kanban::Error::MissingApproval.into());
        }
        let mut state = self.state.lock().map_err(|_| kanban::Error::MoveBusy)?;
        let context = state
            .sources
            .get(&ticket.card.lane)
            .ok_or(kanban::Error::StaleMove)?
            .context
            .clone();
        let entry = state
            .entries
            .get_mut(&ticket.card)
            .filter(|entry| {
                entry.status.operation == ticket.operation
                    && !entry.cancelled.load(Ordering::Acquire)
            })
            .ok_or(kanban::Error::MissingApproval)?;
        let MovePhase::AwaitingConfirmation(plan) = &entry.status.phase else {
            return Err(kanban::Error::MissingApproval.into());
        };
        if plan.context() != &context {
            return Err(kanban::Error::StaleMove.into());
        }
        let permit = entry.permit.take().ok_or(kanban::Error::MissingApproval)?;
        let approved = ConfirmedMove::approved(plan.clone(), permit);
        // Keep the entry awaiting submission while disarming this ticket's Drop.
        drop(state);
        ticket.armed = false;
        Ok(approved)
    }

    pub(in crate::service) fn take_session(
        &self,
        plan: &MovePlan,
    ) -> Result<crate::forgejo::moves::Session> {
        let mut state = self.state.lock().map_err(|_| kanban::Error::MoveBusy)?;
        state
            .entries
            .get_mut(plan.card())
            .filter(|entry| entry.status.operation == plan.operation())
            .and_then(|entry| entry.session.take())
            .ok_or_else(|| kanban::Error::MissingApproval.into())
    }

    /// Consume approval once. Callback ownership freezes routing for the accepted thread.
    pub fn submit(
        &self,
        confirmed: ConfirmedMove,
        read: impl FnMut() -> Result<Fresh> + Send + 'static,
        mutate: impl FnMut(&MovePlan, &Mutation) -> Attempt + Send + 'static,
    ) -> Result<()> {
        confirmed.submit(self.clone(), read, mutate)
    }

    pub(crate) fn accept(&self, plan: &MovePlan) -> Result<ActionSlot> {
        let slot = match ActionSlot::acquire(self.actions.clone()) {
            Ok(slot) => slot,
            Err(error) => {
                self.reject_submission(plan, error);
                return Err(crate::Error::Busy);
            }
        };
        {
            let mut state = self.state.lock().map_err(|_| kanban::Error::MoveBusy)?;
            let source = state
                .sources
                .get_mut(&plan.card().lane)
                .ok_or(kanban::Error::StaleMove)?;
            if source.context != *plan.context() {
                return Err(kanban::Error::StaleMove.into());
            }
            source.epoch = Uuid::new_v4();
            let entry = state
                .entries
                .get_mut(plan.card())
                .ok_or(kanban::Error::MissingApproval)?;
            if entry.cancelled.load(Ordering::Acquire)
                || !matches!(&entry.status.phase, MovePhase::AwaitingConfirmation(held) if held.operation() == plan.operation())
            {
                return Err(kanban::Error::MissingApproval.into());
            }
            entry.status.phase = MovePhase::Writing;
            self.changed.store(true, Ordering::Release);
        }
        Ok(slot)
    }

    pub(crate) fn validate_context(&self, plan: &MovePlan) -> Result<()> {
        let state = self.state.lock().map_err(|_| kanban::Error::MoveBusy)?;
        if state
            .sources
            .get(&plan.card().lane)
            .is_none_or(|source| source.context != *plan.context())
        {
            return Err(kanban::Error::StaleMove.into());
        }
        Ok(())
    }

    fn reject_submission(&self, plan: &MovePlan, error: crate::Error) {
        if let Ok(mut state) = self.state.lock()
            && let Some(entry) = state.entries.get_mut(plan.card())
            && entry.status.operation == plan.operation()
        {
            entry.permit = None;
            entry.status.phase = MovePhase::Refused(Arc::new(error));
            entry.terminal_at = Some(Instant::now());
            self.changed.store(true, Ordering::Release);
        }
    }

    pub(crate) fn finish(&self, completion: Completion, permit: Option<MovePermit>) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let plan = &completion.plan;
        if state
            .sources
            .get(&plan.card().lane)
            .is_none_or(|source| source.context != *plan.context())
        {
            return;
        }
        let Some(entry) = state.entries.get_mut(plan.card()) else {
            return;
        };
        if entry.status.operation != plan.operation()
            || !matches!(&entry.status.phase, MovePhase::Writing)
        {
            return;
        }
        if let Some(record) = &completion.observation {
            entry.status.observed = record.clone();
        }
        let attempted = completion.attempted;
        let lane = plan.card().lane.clone();
        entry.status.phase = MovePhase::Finished(Arc::new(completion));
        entry.terminal_at = Some(Instant::now());
        self.changed.store(true, Ordering::Release);
        if let Some(source) = state.sources.get_mut(&lane) {
            source.invalidated |= attempted;
            source.epoch = Uuid::new_v4();
        }
        // A reader cannot see Finished while the previous card permit is held.
        drop(permit);
    }

    pub(in crate::service) fn read_fences(&self) -> Vec<ReadFence> {
        let Ok(state) = self.state.lock() else {
            return Vec::new();
        };
        state
            .sources
            .iter()
            .map(|(lane, source)| ReadFence {
                epoch: source.epoch,
                lane: lane.clone(),
                source_generation: source.context.source_generation,
            })
            .collect()
    }

    pub fn begin_read(&self, lane: &LaneKey) -> Option<ReadFence> {
        let state = self.state.lock().ok()?;
        let source = state.sources.get(lane)?;
        Some(ReadFence {
            epoch: source.epoch,
            lane: lane.clone(),
            source_generation: source.context.source_generation,
        })
    }

    /// Only an exact-item observation from a read newer than the write is accepted.
    /// Missing rows in a listing are not evidence of closure or deletion.
    pub fn observe(&self, fence: &ReadFence, records: &[Record]) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let Some(source) = state.sources.get(&fence.lane) else {
            return false;
        };
        if source.epoch != fence.epoch
            || source.context.source_generation != fence.source_generation
        {
            return false;
        }
        for record in records {
            let Ok(card) = CardKey::try_from(&record.item.key) else {
                continue;
            };
            if card.lane != fence.lane {
                continue;
            }
            if let Some(entry) = state.entries.get_mut(&card)
                && matches!(entry.status.phase, MovePhase::Finished(_))
                && record.facts.native_state == Some(kanban::facts::NativeStateProvenance::Tracker)
                && record.facts.labels == Some(kanban::facts::LabelCompleteness::Complete)
                && record.item.updated_at >= entry.status.observed.item.updated_at
            {
                entry.status.observed = record.clone();
                self.changed.store(true, Ordering::Release);
            }
        }
        true
    }

    /// Active readers do not yet expose complete label facts. A later genuine
    /// observation may replace a held item, but keeps that evidence unknown.
    pub(in crate::service) fn observe_items(&self, fence: &ReadFence, items: &[crate::Item]) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(source) = state.sources.get(&fence.lane) else {
            return;
        };
        if source.epoch != fence.epoch
            || source.context.source_generation != fence.source_generation
        {
            return;
        }
        for item in items {
            let Ok(card) = CardKey::try_from(&item.key) else {
                continue;
            };
            if card.lane != fence.lane {
                continue;
            }
            if let Some(entry) = state.entries.get_mut(&card)
                && matches!(entry.status.phase, MovePhase::Finished(_))
                && item.updated_at.is_some()
                && item.updated_at > entry.status.observed.item.updated_at
            {
                self.changed.store(true, Ordering::Release);
                entry.status.observed = Record {
                    item: item.clone(),
                    facts: kanban::facts::WorkflowFacts::default(),
                };
            }
        }
    }

    /// The normal scheduler consumes invalidations; this never resubmits an action.
    pub fn take_invalidated(&self) -> Vec<LaneKey> {
        let Ok(mut state) = self.state.lock() else {
            return Vec::new();
        };
        state
            .sources
            .iter_mut()
            .filter_map(|(lane, source)| {
                std::mem::take(&mut source.invalidated).then(|| lane.clone())
            })
            .collect()
    }
}

fn cancel_entry(entry: &mut Entry) {
    if matches!(
        entry.status.phase,
        MovePhase::Preparing | MovePhase::AwaitingConfirmation(_)
    ) {
        if matches!(entry.status.phase, MovePhase::AwaitingConfirmation(_)) {
            entry.terminal_at = Some(Instant::now());
        }
        entry.cancelled.store(true, Ordering::Release);
        entry.permit = None;
        entry.session = None;
        entry.status.phase = MovePhase::Cancelled;
    }
}

pub(crate) struct ActionSlot(Arc<AtomicUsize>);

impl ActionSlot {
    fn acquire(actions: Arc<AtomicUsize>) -> Result<Self> {
        actions
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |held| {
                (held < MAX_ACTIONS).then_some(held + 1)
            })
            .map_err(|_| crate::Error::Busy)?;
        Ok(Self(actions))
    }
}

impl Drop for ActionSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
