//! History uses the existing worker tick. Its failures never replace active status.

pub mod moves;

use super::{Access, Error, Snapshot, Store, retry_wait, schedule};
use crate::{
    Item, Result,
    kanban::history::{Collection, Origin, Read, Sample, Status},
};
use chrono::{DateTime, Utc};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

impl Snapshot {
    /// Immutable board inputs, without changing the active listing used by other views.
    /// History coverage remains in `history`; a failed attempt never changes its success time.
    pub fn board_records(&self, now: DateTime<Utc>) -> Vec<crate::kanban::history::Record> {
        use crate::kanban::{
            facts::WorkflowFacts,
            history::{Record, WINDOW, bound_settled, identity},
        };
        let mut records: HashMap<_, _> = self
            .items
            .iter()
            .map(|item| {
                (
                    identity(item),
                    Record {
                        item: item.clone(),
                        facts: WorkflowFacts::default(),
                    },
                )
            })
            .collect();
        for status in self.history.iter() {
            let Some(mut sample) = status.sample.clone() else {
                continue;
            };
            sample.reconcile(&self.items, now);
            for record in sample.records {
                match records.entry(identity(&record.item)) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(record);
                    }
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        if crate::kanban::facts::prefer(
                            &record.item,
                            &record.facts,
                            &entry.get().item,
                            &entry.get().facts,
                        ) {
                            entry.insert(record);
                        }
                    }
                }
            }
        }
        // A desired destination never changes a card. Only retained observations
        // override a listing that may have started before the action finished.
        for status in self.moves.iter().filter(|status| {
            !matches!(
                status.phase,
                moves::MovePhase::Cancelled
                    | moves::MovePhase::Refused(_)
                    | moves::MovePhase::Unchanged
            )
        }) {
            records.insert(identity(&status.observed.item), status.observed.clone());
        }
        let mut records: Vec<_> = records.into_values().collect();
        // Outcomes remain in `moves` even after their settled cards leave the window.
        bound_settled(&mut records, now - WINDOW);
        records.sort_by_key(|record| record.item.key.canonical());
        records
    }
}

#[derive(Default)]
pub(super) struct History {
    entries: HashMap<Collection, Entry>,
}

struct Entry {
    attempted: Option<Instant>,
    failures: u32,
    provider_failures: u32,
    provider_until: Option<Instant>,
    retry_until: Option<Instant>,
    status: Status,
}

impl History {
    fn entry(&mut self, collection: &Collection) -> &mut Entry {
        self.entries
            .entry(collection.clone())
            .or_insert_with(|| Entry {
                attempted: None,
                failures: 0,
                provider_failures: 0,
                provider_until: None,
                retry_until: None,
                status: Status {
                    collection: collection.clone(),
                    failure: None,
                    origin: None,
                    sample: None,
                    supported: false,
                },
            })
    }

    pub(super) fn load(&mut self, store: &Store) -> Result<()> {
        let cached = store.history()?;
        for entry in self.entries.values_mut() {
            if entry.status.origin == Some(Origin::Cached)
                && !cached
                    .iter()
                    .any(|sample| sample.collection() == &entry.status.collection)
            {
                entry.status.sample = None;
            }
        }
        for sample in cached {
            let entry = self.entry(sample.collection());
            if let Some(held) = &mut entry.status.sample {
                if held.coverage().last_success > sample.coverage().last_success {
                    continue;
                }
                if held.coverage().last_success == sample.coverage().last_success {
                    // Reconciliation changes records, not observation time. Keep removals
                    // from either worker, including memory-only newer-schema suppression.
                    held.records.retain(|record| {
                        sample.records().iter().any(|cached| {
                            crate::kanban::history::identity(&record.item)
                                == crate::kanban::history::identity(&cached.item)
                        })
                    });
                    continue;
                }
            }
            entry.status.sample = Some(sample);
            entry.status.origin = Some(Origin::Cached);
            entry.status.supported = true;
        }
        Ok(())
    }

    pub(super) fn refresh(&mut self) {
        for entry in self.entries.values_mut() {
            entry.attempted = None;
            // Explicit refresh does not bypass a provider's requested delay.
        }
    }

    pub(super) fn provider_until(&self, collection: &Collection) -> Option<Instant> {
        self.entries
            .get(collection)
            .and_then(|entry| entry.provider_until)
    }

    pub(super) fn read(
        &mut self,
        collection: Collection,
        context: ReadContext<'_>,
        fetch: impl FnOnce(&mut Option<Duration>) -> Result<Option<Read>>,
    ) -> bool {
        let ReadContext {
            active,
            now,
            observed_at,
            store,
            visible,
            blocked_until,
        } = context;
        let entry = self.entry(&collection);
        if !schedule::history_due(
            entry.attempted,
            entry
                .retry_until
                .into_iter()
                .chain(entry.provider_until)
                .chain(blocked_until)
                .max(),
            visible,
            now,
        ) {
            return false;
        }
        let mut cooldown = None;
        let read = fetch(&mut cooldown);
        entry.provider_until = cooldown.map(|wait| now + wait);
        if let Err(error @ (Error::Github(_) | Error::Forgejo(_))) = &read {
            entry.provider_failures = entry.provider_failures.saturating_add(1);
            let wait = schedule::backoff(retry_wait(error, cooldown), entry.provider_failures);
            entry.provider_until = Some(now + wait);
        } else if !matches!(read, Err(Error::Cancelled)) {
            entry.provider_failures = 0;
        }
        entry.attempted = Some(now);
        let outcome = read.and_then(|read| {
            let Some(read) = read else {
                entry.status.supported = false;
                return Ok(None);
            };
            entry.status.supported = true;
            let mut sample = Sample::from_read(collection, read, observed_at)?;
            sample.reconcile(active, observed_at);
            match store {
                Some(store) if store.access() == Access::ReadWrite => {
                    sample = store.replace_history(&sample, active, observed_at)?;
                }
                // Unknown newer schemas expose new observations in memory only.
                Some(_) | None => {}
            }
            Ok(Some(sample))
        });
        match outcome {
            Ok(sample) => {
                entry.failures = 0;
                entry.retry_until = None;
                entry.status.failure = None;
                if let Some(sample) = sample {
                    entry.status.origin = Some(if sample.coverage().last_success == observed_at {
                        Origin::Live
                    } else {
                        Origin::Cached
                    });
                    entry.status.sample = Some(sample);
                }
            }
            Err(Error::Cancelled) => {}
            Err(error) => {
                // An absent reader returns None; an error is a failed supported read.
                entry.status.supported = true;
                entry.failures = entry.failures.saturating_add(1);
                let wait = schedule::backoff(retry_wait(&error, cooldown), entry.failures);
                entry.retry_until = Some(now + wait);
                entry.status.failure = Some(Arc::new(error));
            }
        }
        true
    }

    pub(super) fn reconcile(
        &mut self,
        store: Option<&mut Store>,
        active: &[Item],
        now: DateTime<Utc>,
    ) {
        for entry in self.entries.values_mut() {
            if let Some(sample) = &mut entry.status.sample {
                sample.reconcile(active, now);
            }
        }
        if let Some(store) = store.filter(|store| store.access() == Access::ReadWrite)
            && let Err(error) = store.reconcile_history(active, now)
        {
            let error = Arc::new(error);
            for entry in self.entries.values_mut() {
                entry.status.failure = Some(error.clone());
            }
        }
    }

    pub(super) fn publish(&mut self, snapshot: &mut Snapshot, now: DateTime<Utc>) {
        let mut statuses = Vec::new();
        for entry in self.entries.values_mut() {
            if let Some(sample) = &mut entry.status.sample {
                sample.reconcile(&snapshot.items, now);
            }
            statuses.push(entry.status.clone());
        }
        statuses.sort_by_key(|status| status.collection.source().canonical());
        snapshot.history = Arc::new(statuses);
    }
}

pub(super) struct ReadContext<'a> {
    pub active: &'a [Item],
    pub blocked_until: Option<Instant>,
    pub now: Instant,
    pub observed_at: DateTime<Utc>,
    pub store: Option<&'a mut Store>,
    pub visible: bool,
}

impl super::Worker {
    pub(super) fn sync_history(&mut self) -> bool {
        let Some(info) = &self.snapshot.repo else {
            return false;
        };
        let mut changed = false;
        for (source, supported) in super::sources(info) {
            if !supported
                || !matches!(
                    source.provider,
                    crate::Provider::Github | crate::Provider::Forgejo
                )
            {
                continue;
            }
            let blocked_until = self
                .retry
                .iter()
                .filter(|(key, _)| *key == source)
                .map(|(_, until)| *until)
                .max();
            let scheme = crate::forge::Scheme::of_source(Some(info), &source);
            changed |= self.history.read(
                Collection::Repository(source.clone()),
                ReadContext {
                    active: &self.snapshot.items,
                    blocked_until,
                    now: Instant::now(),
                    observed_at: Utc::now(),
                    store: self.store.as_mut(),
                    visible: self.visible.load(std::sync::atomic::Ordering::Relaxed),
                },
                |cooldown| match source.provider {
                    crate::Provider::Forgejo => {
                        super::forgejo_source::history((&source, scheme), &self.cancelled, cooldown)
                            .map(Some)
                    }
                    _ => Ok(None), // GitHub's reader arrives separately.
                },
            );
        }
        if changed {
            self.history
                .reconcile(self.store.as_mut(), &self.snapshot.items, Utc::now());
            self.history.publish(&mut self.snapshot, Utc::now());
        }
        changed
    }
}
