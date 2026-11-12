//! Bounded samples of settled tracker observations, separate from active listings.

use super::{
    Error, Result,
    facts::WorkflowFacts,
    identity::{LaneKey, normalized_source},
};
use crate::{Item, Provider, SourceKey};
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
};

pub const MAX_RECORDS: usize = 100;
pub const MAX_ACCOUNT_RECORDS: usize = 200;
pub const WINDOW: TimeDelta = TimeDelta::days(30);

/// An account collection is not a repository lane. Its rows retain their own keys.
#[derive(Clone, Debug, Eq, Serialize, Deserialize)]
pub enum Collection {
    ForgejoAccount { host: String },
    Repository(SourceKey),
}

impl PartialEq for Collection {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Repository(left), Self::Repository(right)) => {
                normalized_source(left) == normalized_source(right)
            }
            (Self::ForgejoAccount { host: left }, Self::ForgejoAccount { host: right }) => {
                left.eq_ignore_ascii_case(right)
            }
            _ => false,
        }
    }
}

impl Hash for Collection {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Repository(source) => normalized_source(source).hash(state),
            Self::ForgejoAccount { host } => host.to_ascii_lowercase().hash(state),
        }
    }
}

impl Collection {
    pub(crate) fn key(&self) -> String {
        normalized_source(&self.source()).canonical()
    }

    pub fn source(&self) -> SourceKey {
        match self {
            Self::Repository(source) => source.clone(),
            Self::ForgejoAccount { host } => crate::forgejo::search::account_source(host),
        }
    }

    pub fn limit(&self) -> usize {
        match self {
            Self::Repository(_) => MAX_RECORDS,
            Self::ForgejoAccount { .. } => MAX_ACCOUNT_RECORDS,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        match self {
            Self::Repository(source) => LaneKey::try_from(source).map(|_| ()),
            Self::ForgejoAccount { host } => LaneKey::try_from(&SourceKey {
                provider: Provider::Forgejo,
                host: host.clone(),
                repository: "account/validation".into(),
            })
            .map(|_| ()),
        }
    }

    fn contains(&self, source: &SourceKey) -> Result<bool> {
        let lane = LaneKey::try_from(source)?;
        Ok(match self {
            Self::Repository(held) => LaneKey::try_from(held)? == lane,
            Self::ForgejoAccount { host } => {
                source.provider == Provider::Forgejo && source.host.eq_ignore_ascii_case(host)
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub facts: WorkflowFacts,
    pub item: Item,
}

/// `complete` means every required page succeeded, not that the tracker is exhausted.
/// A successful read stopped at its intentional cap has `truncated = true`.
pub struct Read {
    pub complete: bool,
    pub cutoff: DateTime<Utc>,
    pub records: Vec<Record>,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    pub cutoff: DateTime<Utc>,
    pub last_success: DateTime<Utc>,
    pub limit: usize,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Cached,
    Live,
}

/// Failure describes the latest attempt; sample coverage always describes its last success.
#[derive(Clone, Debug)]
pub struct Status {
    pub collection: Collection,
    pub failure: Option<std::sync::Arc<crate::Error>>,
    pub origin: Option<Origin>,
    pub sample: Option<Sample>,
    pub supported: bool,
}

/// Only validated, complete bounded reads become replaceable samples.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    pub(crate) collection: Collection,
    pub(crate) coverage: Coverage,
    pub(crate) records: Vec<Record>,
}

impl Sample {
    pub fn from_read(collection: Collection, read: Read, now: DateTime<Utc>) -> Result<Self> {
        collection.validate()?;
        if !read.complete {
            return Err(Error::HistoryIncomplete);
        }
        if read.cutoff > now {
            return Err(Error::HistoryWindow);
        }
        // Two repository feeds (issues and pulls), or one account feed.
        if read.records.len() > MAX_ACCOUNT_RECORDS {
            return Err(Error::HistoryLimit {
                max: MAX_ACCOUNT_RECORDS,
            });
        }
        for record in &read.records {
            if !collection.contains(&record.item.key.source)? {
                return Err(Error::Route);
            }
            if !settled(&record.item) || record.item.updated_at.is_none() {
                return Err(Error::HistoryRecord);
            }
        }
        let mut sample = Self {
            coverage: Coverage {
                cutoff: read.cutoff.max(now - WINDOW),
                last_success: now,
                limit: collection.limit(),
                truncated: read.truncated,
            },
            collection,
            records: read.records,
        };
        sample.coverage.truncated |= bound_settled(&mut sample.records, sample.coverage.cutoff);
        Ok(sample)
    }

    pub fn collection(&self) -> &Collection {
        &self.collection
    }
    pub fn coverage(&self) -> &Coverage {
        &self.coverage
    }
    pub fn records(&self) -> &[Record] {
        &self.records
    }

    /// Expiry and known reopens remove history only. Missing active rows say nothing.
    pub(crate) fn reconcile(&mut self, active: &[Item], now: DateTime<Utc>) {
        let open: HashMap<_, _> = active
            .iter()
            .filter(|item| matches!(item.state.as_str(), "open" | "draft"))
            .map(|item| (identity(item), item.updated_at))
            .collect();
        self.records.retain(|record| {
            let updated = record.item.updated_at;
            updated.is_some_and(|at| at >= now - WINDOW)
                && !open
                    .get(&identity(&record.item))
                    .is_some_and(|at| at.is_none() || *at >= updated)
        });
    }
}

/// Select the newest settled records per source without trimming active observations.
/// Shared by provider samples and the board's merged move observations.
pub(crate) fn bound_settled(records: &mut Vec<Record>, cutoff: DateTime<Utc>) -> bool {
    records.retain(|record| {
        !settled(&record.item) || record.item.updated_at.is_some_and(|at| at >= cutoff)
    });
    records.sort_by(|a, b| {
        b.item
            .updated_at
            .cmp(&a.item.updated_at)
            .then_with(|| a.item.key.canonical().cmp(&b.item.key.canonical()))
    });
    let mut seen = HashSet::new();
    let mut sources = HashMap::new();
    let mut truncated = false;
    records.retain(|record| {
        if !seen.insert(identity(&record.item)) {
            return false;
        }
        if !settled(&record.item) {
            return true;
        }
        let kept = sources
            .entry(normalized_source(&record.item.key.source))
            .or_insert(0);
        if *kept >= MAX_RECORDS {
            truncated = true;
            return false;
        }
        *kept += 1;
        true
    });
    truncated
}

pub(crate) fn settled(item: &Item) -> bool {
    matches!(
        item.key.source.provider,
        Provider::Github | Provider::Forgejo
    ) && (item.state == "closed"
        || (item.state == "merged"
            && (item.pull_request.is_some() || item.key.native_id.starts_with("pr/"))))
}

pub(crate) fn identity(item: &Item) -> (String, String) {
    (
        normalized_source(&item.key.source).canonical(),
        item.key.native_id.clone(),
    )
}
