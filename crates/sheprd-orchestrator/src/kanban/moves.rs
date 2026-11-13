//! Exact read-only plans and non-cloneable approval. No provider writer lives here.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

pub use super::error::PermissionStep;

use super::{
    BoardColumn, Error, Result,
    config::LabelMap,
    facts::{self, LabelCompleteness, MappingWarning, NativeStateProvenance, WorkflowFacts},
    history::Record,
    identity::{CardKey, LaneKey, Route},
};
use crate::{Item, Provider};

/// Routing metadata only. Credentials must never be placed in this value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Context {
    pub account: String,
    pub account_generation: u64,
    pub api: url::Url,
    pub board_generation: u64,
    pub mapping: LabelMap,
    pub mapping_generation: u64,
    pub route: Route<Uuid>,
    pub source_generation: Uuid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LabelId {
    // keep-sorted start
    Forgejo(u64),
    Github(String),
    // keep-sorted end
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub exclusive: bool,
    pub id: LabelId,
    pub name: String,
}

/// The two existing definitions resolved by the provider, never created by preflight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Labels {
    pub in_progress: Option<Label>,
    pub in_review: Option<Label>,
}

/// Stable provider database IDs, not the reusable repository path or issue number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Identity {
    Forgejo { repository: u64, issue: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fresh {
    pub context: Context,
    pub facts: WorkflowFacts,
    pub identity: Identity,
    pub item: Item,
    pub labels: Labels,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeState {
    Open,
    Closed,
}

impl NativeState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mutation {
    RemoveLabel(LabelId),
    AddLabel(LabelId),
    SetState(NativeState),
}

#[derive(Clone, Debug)]
pub struct Proposal {
    pub board_generation: u64,
    pub card: CardKey,
    pub destination: BoardColumn,
    pub destination_lane: LaneKey,
}

/// Immutable approval inputs. The original item remains the displayed observation.
#[derive(Debug)]
pub struct MovePlan {
    before: Fresh,
    card: CardKey,
    destination: BoardColumn,
    mutations: Vec<Mutation>,
    operation: Uuid,
    original_column: BoardColumn,
}

impl MovePlan {
    /// Read-only. Same-column requests have no plan unless resolving a visible conflict.
    pub fn prepare(proposal: Proposal, fresh: Fresh) -> Result<Option<Self>> {
        if proposal.card.lane != proposal.destination_lane {
            return Err(Error::CrossLane);
        }
        validate_fresh(&proposal.card, &fresh)?;
        if proposal.board_generation != fresh.context.board_generation {
            return Err(Error::StaleMove);
        }
        let placement = facts::place(&fresh.item, &fresh.facts, &fresh.context.mapping);
        if placement.column == proposal.destination
            && !placement
                .warnings
                .contains(&MappingWarning::ConflictingLabels)
        {
            return Ok(None);
        }
        let mut mutations = Vec::new();
        let desired = desired_labels(proposal.destination);
        let definitions = [&fresh.labels.in_progress, &fresh.labels.in_review];
        for (label, wanted) in definitions.into_iter().zip(desired) {
            if let Some(label) = label
                && !wanted
                && fresh.item.labels.contains(&label.name)
            {
                mutations.push(Mutation::RemoveLabel(label.id.clone()));
            }
        }
        for (label, wanted) in definitions.into_iter().zip(desired) {
            if wanted {
                let label = label.as_ref().ok_or(Error::MissingLabel)?;
                if !fresh.item.labels.contains(&label.name) {
                    mutations.push(Mutation::AddLabel(label.id.clone()));
                }
            }
        }
        let state = desired_state(proposal.destination);
        if fresh.item.state != state.as_str() {
            mutations.push(Mutation::SetState(state));
        }
        if mutations.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self {
            before: fresh,
            card: proposal.card,
            destination: proposal.destination,
            mutations,
            operation: Uuid::new_v4(),
            original_column: placement.column,
        }))
    }

    pub fn card(&self) -> &CardKey {
        &self.card
    }
    pub fn context(&self) -> &Context {
        &self.before.context
    }
    pub(crate) fn identity(&self) -> Identity {
        self.before.identity
    }
    pub fn destination(&self) -> BoardColumn {
        self.destination
    }
    pub fn original_column(&self) -> BoardColumn {
        self.original_column
    }
    pub fn mutations(&self) -> &[Mutation] {
        &self.mutations
    }
    /// The exact definition already resolved during preflight, for native confirmation.
    pub fn label_name(&self, id: &LabelId) -> Option<&str> {
        [
            &self.before.labels.in_progress,
            &self.before.labels.in_review,
        ]
        .into_iter()
        .flatten()
        .find(|label| &label.id == id)
        .map(|label| label.name.as_str())
    }
    pub fn operation(&self) -> Uuid {
        self.operation
    }
    pub(crate) fn set_operation(&mut self, operation: Uuid) {
        self.operation = operation;
    }
    pub fn original(&self) -> Record {
        Record {
            item: self.before.item.clone(),
            facts: self.before.facts.clone(),
        }
    }

    /// Label order is irrelevant, but every observed label remains a precondition.
    pub fn revalidate(&self, fresh: &Fresh) -> Result<()> {
        validate_fresh(&self.card, fresh)?;
        if fresh.context != self.before.context
            || fresh.identity != self.before.identity
            || fresh.item.state != self.before.item.state
            || fresh.labels != self.before.labels
            || label_set(fresh) != label_set(&self.before)
        {
            return Err(Error::StaleMove);
        }
        Ok(())
    }

    pub(crate) fn observation(&self, fresh: Fresh) -> Result<Record> {
        validate_fresh(&self.card, &fresh)?;
        if fresh.context != self.before.context || fresh.identity != self.before.identity {
            return Err(Error::StaleMove);
        }
        Ok(Record {
            item: fresh.item,
            facts: fresh.facts,
        })
    }

    pub(crate) fn reached(&self, record: &Record) -> bool {
        record.item.state == desired_state(self.destination).as_str()
            && [
                self.context().mapping.in_progress_label(),
                self.context().mapping.in_review_label(),
            ]
            .into_iter()
            .map(|name| record.item.labels.iter().any(|held| held == name))
            .eq(desired_labels(self.destination))
    }
}

fn desired_state(destination: BoardColumn) -> NativeState {
    if destination == BoardColumn::Done {
        NativeState::Closed
    } else {
        NativeState::Open
    }
}

fn desired_labels(destination: BoardColumn) -> [bool; 2] {
    [
        destination == BoardColumn::InProgress,
        destination == BoardColumn::InReview,
    ]
}

fn label_set(fresh: &Fresh) -> HashSet<&str> {
    fresh.item.labels.iter().map(String::as_str).collect()
}

fn validate_fresh(card: &CardKey, fresh: &Fresh) -> Result<()> {
    if CardKey::try_from(&fresh.item.key)? != *card {
        return Err(Error::Route);
    }
    fresh.context.route.validate(&card.item.source)?;
    let api = &fresh.context.api;
    let host = api.host_str().unwrap_or_default();
    let authority = api
        .port()
        .map_or_else(|| host.to_owned(), |port| format!("{host}:{port}"));
    if api.scheme() != "https"
        || !api.username().is_empty()
        || api.password().is_some()
        || api.query().is_some()
        || api.fragment().is_some()
        || !match card.item.source.provider {
            Provider::Forgejo => authority.eq_ignore_ascii_case(&card.item.source.host),
            Provider::Github => {
                card.item.source.host.eq_ignore_ascii_case("github.com")
                    && authority == "api.github.com"
                    && api.path() == "/graphql"
            }
            _ => false,
        }
    {
        return Err(Error::Route);
    }
    if fresh.context.account.trim().is_empty() {
        return Err(Error::MovePermission {
            step: PermissionStep::AccountIdentity,
            status: None,
        });
    }
    match (card.item.source.provider, fresh.identity) {
        (Provider::Forgejo, Identity::Forgejo { repository, issue })
            if repository > 0 && issue > 0 => {}
        _ => return Err(Error::Route),
    }
    if fresh.item.pull_request.is_some()
        || fresh
            .item
            .key
            .native_id
            .parse::<u64>()
            .ok()
            .is_none_or(|number| number == 0 || number.to_string() != fresh.item.key.native_id)
    {
        return Err(Error::ReadOnlyMove);
    }
    if !matches!(fresh.item.state.as_str(), "open" | "closed")
        || fresh.facts.native_state != Some(NativeStateProvenance::Tracker)
        || fresh.facts.labels != Some(LabelCompleteness::Complete)
    {
        return Err(Error::ReadOnlyMove);
    }
    for (label, name) in [
        (
            &fresh.labels.in_progress,
            fresh.context.mapping.in_progress_label(),
        ),
        (
            &fresh.labels.in_review,
            fresh.context.mapping.in_review_label(),
        ),
    ] {
        let Some(label) = label else {
            if fresh.item.labels.iter().any(|held| held == name) {
                return Err(Error::MissingLabel);
            }
            continue;
        };
        if label.name != name || label.exclusive {
            return Err(Error::UnsafeLabel);
        }
        match (&label.id, card.item.source.provider) {
            (LabelId::Forgejo(id), Provider::Forgejo) if *id > 0 => {}
            (LabelId::Github(id), Provider::Github)
                if !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control) => {}
            _ => return Err(Error::UnsafeLabel),
        }
    }
    if let (Some(progress), Some(review)) = (&fresh.labels.in_progress, &fresh.labels.in_review)
        && progress.id == review.id
    {
        return Err(Error::UnsafeLabel);
    }
    Ok(())
}

/// A shared bounded set of in-flight identities, not a service or transport registry.
#[derive(Debug)]
pub struct MoveGate {
    capacity: usize,
    cards: Mutex<HashSet<(LaneKey, String)>>,
}

impl MoveGate {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            cards: Mutex::default(),
        }
    }

    pub fn acquire(self: &Arc<Self>, card: &CardKey) -> Result<MovePermit> {
        if LaneKey::try_from(&card.item.source)? != card.lane {
            return Err(Error::Route);
        }
        let identity = (card.lane.clone(), card.item.native_id.clone());
        let mut cards = self.cards.lock().map_err(|_| Error::MoveBusy)?;
        if cards.len() >= self.capacity || !cards.insert(identity.clone()) {
            return Err(Error::MoveBusy);
        }
        Ok(MovePermit {
            gate: self.clone(),
            identity,
        })
    }
}

#[derive(Debug)]
pub struct MovePermit {
    gate: Arc<MoveGate>,
    identity: (LaneKey, String),
}

impl MovePermit {
    fn matches(&self, plan: &MovePlan) -> bool {
        self.identity == (plan.card.lane.clone(), plan.card.item.native_id.clone())
    }
}

impl Drop for MovePermit {
    fn drop(&mut self) {
        if let Ok(mut cards) = self.gate.cards.lock() {
            cards.remove(&self.identity);
        }
    }
}

/// Private approval built only after consuming a preparation-issued ticket.
/// Neither a command nor a drop can construct it.
#[derive(Debug)]
pub struct ConfirmedMove {
    permit: MovePermit,
    plan: Arc<MovePlan>,
}

impl ConfirmedMove {
    pub(crate) fn approved(plan: Arc<MovePlan>, permit: MovePermit) -> Self {
        Self { permit, plan }
    }

    pub(crate) fn plan(&self) -> &MovePlan {
        &self.plan
    }

    #[cfg(test)]
    pub(crate) fn from_dialog(plan: Arc<MovePlan>, permit: MovePermit) -> Self {
        Self { permit, plan }
    }

    /// The only production execution path consumes approval and checks lifecycle admission.
    pub(crate) fn submit(
        self,
        moves: crate::service::Moves,
        mut read: impl FnMut() -> crate::Result<Fresh> + Send + 'static,
        mutate: impl FnMut(&MovePlan, &Mutation) -> Attempt + Send + 'static,
    ) -> crate::Result<()> {
        if !self.permit.matches(&self.plan) {
            return Err(Error::Route.into());
        }
        let slot = moves.accept(&self.plan)?;
        let plan = self.plan.clone();
        let worker = moves.clone();
        let spawned = crate::service::spawn_action(move |_| {
            let _slot = slot;
            let completion = self.perform(
                || {
                    let fresh = read()?;
                    worker.validate_context(&self.plan)?;
                    Ok(fresh)
                },
                mutate,
            );
            // Keep the gate occupied until publication, including the final read.
            worker.finish(completion, Some(self.permit));
        });
        if let Err(error) = spawned {
            moves.finish(Completion::refused(plan, crate::Error::Worker(error)), None);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn perform_for_test(
        self,
        read: impl FnMut() -> crate::Result<Fresh>,
        mutate: impl FnMut(&MovePlan, &Mutation) -> Attempt,
    ) -> Completion {
        self.perform(read, mutate)
    }

    fn perform(
        &self,
        mut read: impl FnMut() -> crate::Result<Fresh>,
        mut mutate: impl FnMut(&MovePlan, &Mutation) -> Attempt,
    ) -> Completion {
        let plan = &self.plan;
        if !self.permit.matches(plan) {
            return Completion::refused(plan.clone(), Error::Route.into());
        }
        let preflight = read().and_then(|fresh| plan.revalidate(&fresh).map_err(Into::into));
        if let Err(error) = preflight {
            return Completion::refused(plan.clone(), error);
        }
        let mut completion = Completion {
            applied: Vec::new(),
            attempted: false,
            observation: None,
            outcome: Outcome::Observed,
            plan: plan.clone(),
            read_error: None,
        };
        for mutation in plan.mutations() {
            completion.attempted = true;
            match mutate(plan, mutation) {
                Attempt::Applied => completion.applied.push(mutation.clone()),
                Attempt::Refused(error) => {
                    completion.outcome = if completion.applied.is_empty() {
                        Outcome::Refused(error)
                    } else {
                        Outcome::Partial {
                            failed: mutation.clone(),
                            error,
                        }
                    };
                    break;
                }
                Attempt::Unknown(error) => {
                    completion.outcome = Outcome::Unknown {
                        failed: Some(mutation.clone()),
                        error,
                    };
                    break;
                }
            }
        }
        // A final read cannot turn an ambiguous request into a safe automatic retry.
        match read().and_then(|fresh| plan.observation(fresh).map_err(Into::into)) {
            Ok(record) => {
                if matches!(completion.outcome, Outcome::Observed) && plan.reached(&record) {
                    completion.outcome = Outcome::Verified;
                }
                completion.observation = Some(record);
            }
            Err(error) => {
                let error = Arc::new(error);
                if matches!(completion.outcome, Outcome::Observed) {
                    completion.outcome = Outcome::Unknown {
                        failed: None,
                        error: error.clone(),
                    };
                }
                completion.read_error = Some(error);
            }
        }
        completion
    }
}

/// Providers must distinguish a definite refusal from a possibly accepted request.
#[derive(Debug)]
pub enum Attempt {
    Applied,
    Refused(Arc<crate::Error>),
    Unknown(Arc<crate::Error>),
}

#[derive(Clone, Debug)]
pub enum Outcome {
    Refused(Arc<crate::Error>),
    Partial {
        failed: Mutation,
        error: Arc<crate::Error>,
    },
    Unknown {
        failed: Option<Mutation>,
        error: Arc<crate::Error>,
    },
    Verified,
    Observed,
}

#[derive(Clone, Debug)]
pub struct Completion {
    pub applied: Vec<Mutation>,
    pub attempted: bool,
    pub observation: Option<Record>,
    pub outcome: Outcome,
    pub plan: Arc<MovePlan>,
    pub read_error: Option<Arc<crate::Error>>,
}

impl Completion {
    pub(crate) fn refused(plan: Arc<MovePlan>, error: crate::Error) -> Self {
        Self {
            applied: Vec::new(),
            attempted: false,
            observation: None,
            outcome: Outcome::Refused(Arc::new(error)),
            plan,
            read_error: None,
        }
    }
}
