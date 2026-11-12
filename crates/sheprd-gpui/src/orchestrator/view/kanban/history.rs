//! Prepare safe history metadata and localized lane text off the render path.
use crate::i18n::fl;
use chrono::{DateTime, Utc};
use sheprd_control_protocol::protocol::orchestrator::HistoryCoverage;
use sheprd_orchestrator::{
    Item, Provider, Snapshot, SourceKey,
    kanban::{
        history::{Collection, Origin, Status, WINDOW},
        identity::{CardKey, LaneKey},
    },
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Default)]
pub(super) struct Loaded {
    cached: usize,
    total: usize,
}

struct Observation<'a> {
    item: &'a Item,
    origin: Option<Origin>,
}

pub(super) struct Observations<'a> {
    records: HashMap<(LaneKey, String), Vec<Observation<'a>>>,
}

impl<'a> Observations<'a> {
    pub(super) fn new(
        inputs: impl IntoIterator<Item = (bool, &'a Snapshot)>,
        checkouts: &HashSet<LaneKey>,
        now: DateTime<Utc>,
    ) -> Self {
        let mut records: HashMap<_, Vec<_>> = HashMap::new();
        for (checkout, snapshot) in inputs {
            for status in snapshot.history.iter() {
                let Some(sample) = &status.sample else {
                    continue;
                };
                for record in sample.records() {
                    let item = &record.item;
                    let Ok(lane) = LaneKey::try_from(&item.key.source) else {
                        continue;
                    };
                    if (!checkout && checkouts.contains(&lane))
                        || !item.updated_at.is_some_and(|at| at >= now - WINDOW)
                    {
                        continue;
                    }
                    records
                        .entry((lane, item.key.native_id.clone()))
                        .or_default()
                        .push(Observation {
                            item,
                            origin: status.origin,
                        });
                }
            }
        }
        Self { records }
    }

    /// Projection calls this once per normalized winner, before filters and caps.
    pub(super) fn count(&self, lane: &LaneKey, item: &Item, loaded: &mut HashMap<LaneKey, Loaded>) {
        let Some(records) = self
            .records
            .get(&(lane.clone(), item.key.native_id.clone()))
        else {
            return;
        };
        let mut matching = records
            .iter()
            .filter(|record| {
                record.item.updated_at == item.updated_at && record.item.state == item.state
            })
            .peekable();
        if matching.peek().is_none() {
            return;
        }
        let cached = matching.all(|record| record.origin == Some(Origin::Cached));
        let loaded = loaded.entry(lane.clone()).or_default();
        loaded.total += 1;
        loaded.cached += usize::from(cached);
    }
}

pub(super) fn coverage(source: &SourceKey, snapshot: &Snapshot, account: bool) -> HistoryCoverage {
    let collection = if account {
        Collection::ForgejoAccount {
            host: source.host.clone(),
        }
    } else {
        Collection::Repository(source.clone())
    };
    let status = snapshot
        .history
        .iter()
        .find(|status| status.collection == collection);
    let sample = status.and_then(|status| status.sample.as_ref());
    let bounds = sample.map(|sample| sample.coverage());
    HistoryCoverage {
        collection: collection.source().canonical(),
        collection_loaded: sample.map_or(0, |sample| sample.records().len()),
        cutoff: bounds.map(|coverage| coverage.cutoff.to_rfc3339()),
        failed: snapshot.history_error.is_some()
            || status.is_some_and(|status| status.failure.is_some()),
        last_success: bounds.map(|coverage| coverage.last_success.to_rfc3339()),
        limit: bounds.map_or(collection.limit(), |coverage| coverage.limit),
        loaded: sample.map_or(0, |sample| {
            let lane = LaneKey::try_from(source).ok();
            sample
                .records()
                .iter()
                .filter(|record| LaneKey::try_from(&record.item.key.source).ok() == lane)
                .count()
        }),
        origin: status.and_then(|status| {
            status.origin.map(|origin| match origin {
                Origin::Cached => "cached".into(),
                Origin::Live => "live".into(),
            })
        }),
        scope: if account { "account" } else { "repository" }.into(),
        supported: status.map_or(source.provider == Provider::Forgejo, |status| {
            status.supported || status.failure.is_some()
        }),
        truncated: bounds.is_some_and(|coverage| coverage.truncated),
    }
}

pub(super) fn complete(key: &CardKey, snapshot: &Snapshot, account: bool) -> bool {
    let held = snapshot.history.iter().any(|status| {
        status.sample.as_ref().is_some_and(|sample| {
            sample.records().iter().any(|record| {
                record.item.key.native_id == key.item.native_id
                    && LaneKey::try_from(&record.item.key.source).ok().as_ref() == Some(&key.lane)
            })
        })
    });
    // Only history-backed cards depend on history freshness for removal.
    // A complete active listing remains authoritative for active-only cards.
    if !held {
        return true;
    }
    let coverage = coverage(&key.item.source, snapshot, account);
    !coverage.failed && (!coverage.supported || coverage.last_success.is_some())
}

pub(super) fn text(coverage: &[HistoryCoverage], loaded: Loaded) -> String {
    if coverage.iter().any(|coverage| coverage.failed) {
        fl!("kanban-history-failed")
    } else if let Some(coverage) = coverage
        .iter()
        .filter(|coverage| coverage.truncated)
        .min_by_key(|coverage| (&coverage.scope, coverage.limit))
    {
        if coverage.scope == "account" {
            fl!("kanban-history-account-partial", limit = coverage.limit)
        } else {
            fl!("kanban-history-partial", limit = coverage.limit)
        }
    } else if coverage
        .iter()
        .any(|coverage| coverage.last_success.is_some())
    {
        let all_cached = if loaded.total == 0 {
            coverage
                .iter()
                .filter(|coverage| coverage.last_success.is_some())
                .all(|coverage| coverage.origin.as_deref() == Some("cached"))
        } else {
            loaded.cached == loaded.total
        };
        if all_cached {
            fl!("kanban-history-cached", count = loaded.total)
        } else if loaded.cached > 0 {
            fl!(
                "kanban-history-mixed",
                count = loaded.total,
                cached = loaded.cached
            )
        } else {
            fl!("kanban-history-loaded", count = loaded.total)
        }
    } else if coverage.iter().any(|coverage| coverage.supported) {
        fl!("kanban-history-pending")
    } else {
        fl!("kanban-history-unavailable")
    }
}

pub(super) fn hint(coverage: &[HistoryCoverage]) -> String {
    let mut lines = vec![fl!("kanban-history-window")];
    for coverage in coverage {
        if let (Some(cutoff), Some(at)) = (&coverage.cutoff, &coverage.last_success) {
            lines.push(fl!(
                "kanban-history-detail",
                cutoff = cutoff.clone(),
                at = at.clone(),
                count = coverage.loaded,
                limit = coverage.limit
            ));
        }
        if coverage.scope == "account" {
            lines.push(fl!(
                "kanban-history-account",
                count = coverage.collection_loaded,
                limit = coverage.limit
            ));
        }
    }
    lines.join("\n")
}

pub(super) fn source(status: &Status) -> Option<SourceKey> {
    match &status.collection {
        Collection::Repository(source) => Some(source.clone()),
        Collection::ForgejoAccount { .. } => None,
    }
}
