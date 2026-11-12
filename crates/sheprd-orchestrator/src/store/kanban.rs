//! Additive history storage. No history operation deletes an active row or a run.

use super::{Store, decode, encode};
use crate::{
    Item, Result,
    kanban::{
        facts::{NativeStateProvenance, WorkflowFacts},
        history::{Collection, Coverage, Record, Sample},
    },
};
use chrono::{DateTime, Utc};
use rusqlite::{Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub(super) const SCHEMA_V4: &str = r#"
CREATE TABLE IF NOT EXISTS kanban_history_samples (
    collection TEXT PRIMARY KEY NOT NULL,
    collection_json TEXT NOT NULL,
    coverage_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS kanban_history (
    collection TEXT NOT NULL,
    canonical_key TEXT NOT NULL,
    item_json TEXT NOT NULL,
    PRIMARY KEY (collection, canonical_key)
);
CREATE TABLE IF NOT EXISTS kanban_history_facts (
    collection TEXT NOT NULL,
    canonical_key TEXT NOT NULL,
    facts_json TEXT NOT NULL,
    PRIMARY KEY (collection, canonical_key)
);
"#;

#[derive(Serialize, Deserialize)]
struct StoredItem {
    version: u32,
    item: Item,
}

#[derive(Serialize, Deserialize)]
struct StoredFacts {
    version: u32,
    #[serde(default)]
    facts: Option<WorkflowFacts>,
}

impl Store {
    /// The last successful samples, including their original success times.
    /// Missing history tables in an unknown newer schema are not an empty live read.
    pub fn history(&self) -> Result<Vec<Sample>> {
        let transaction = self.connection.unchecked_transaction()?;
        let samples = read_samples(&transaction)?;
        if self.access() == crate::Access::ReadWrite {
            for sample in &samples {
                if collection_keys(&transaction, sample.collection())? != [sample.collection.key()]
                {
                    write_sample(&transaction, sample)?;
                }
            }
        }
        transaction.commit()?;
        Ok(samples)
    }

    /// A complete bounded sample and its coverage commit together. A refusal
    /// leaves the previous sample, active checkpoints, and run ownership intact.
    pub fn replace_history(
        &mut self,
        sample: &Sample,
        active: &[Item],
        now: DateTime<Utc>,
    ) -> Result<Sample> {
        self.writable()?;
        let mut sample = sample.clone();
        // Include persisted active observations even when a capped in-memory
        // listing omits them. Only newer known-open observations suppress history.
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(newer) = read_samples(&transaction)?.into_iter().find(|cached| {
            cached.collection() == sample.collection()
                && cached.coverage().last_success > sample.coverage().last_success
        }) {
            sample = newer;
        }
        let stored = active_items(&transaction)?;
        sample.reconcile(&stored, now);
        sample.reconcile(active, now);
        write_sample(&transaction, &sample)?;
        transaction.commit()?;
        Ok(sample)
    }

    /// Expiry affects only this cache. Newer schemas remain completely unwritten.
    pub fn expire_history(&mut self, now: DateTime<Utc>) -> Result<()> {
        self.reconcile_history(&[], now)
    }

    /// Positive observations from capped listings suppress stale history without
    /// replacing any active row or checkpoint. Missing rows prove nothing.
    pub fn reconcile_history(&mut self, observed: &[Item], now: DateTime<Utc>) -> Result<()> {
        self.writable()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let samples = read_samples(&transaction)?;
        let active = active_items(&transaction)?;
        for mut sample in samples {
            let before = sample.records.clone();
            sample.reconcile(&active, now);
            sample.reconcile(observed, now);
            if sample.records != before
                || collection_keys(&transaction, sample.collection())? != [sample.collection.key()]
            {
                write_sample(&transaction, &sample)?;
            }
        }
        transaction.commit()?;
        Ok(())
    }
}

fn collection_keys(
    connection: &rusqlite::Connection,
    collection: &Collection,
) -> Result<Vec<String>> {
    let mut statement = connection.prepare(
        "SELECT collection, collection_json FROM kanban_history_samples ORDER BY collection",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut keys = Vec::new();
    for row in rows {
        let (key, json) = row?;
        if decode::<Collection>("collection_json", &json)? == *collection {
            keys.push(key);
        }
    }
    Ok(keys)
}

fn write_sample(transaction: &Transaction<'_>, sample: &Sample) -> Result<()> {
    let key = sample.collection.key();
    for alias in collection_keys(transaction, sample.collection())? {
        transaction.execute("DELETE FROM kanban_history WHERE collection = ?1", [&alias])?;
        transaction.execute(
            "DELETE FROM kanban_history_facts WHERE collection = ?1",
            [&alias],
        )?;
        transaction.execute(
            "DELETE FROM kanban_history_samples WHERE collection = ?1",
            [&alias],
        )?;
    }
    transaction.execute(
        "INSERT INTO kanban_history_samples (collection, collection_json, coverage_json) VALUES (?1, ?2, ?3) \
         ON CONFLICT(collection) DO UPDATE SET collection_json = excluded.collection_json, coverage_json = excluded.coverage_json",
        params![key, encode("collection_json", &sample.collection)?, encode("coverage_json", &sample.coverage)?],
    )?;
    for record in &sample.records {
        let item = StoredItem {
            version: 1,
            item: record.item.clone(),
        };
        let facts = StoredFacts {
            version: 1,
            facts: Some(record.facts.clone()),
        };
        transaction.execute(
            "INSERT INTO kanban_history (collection, canonical_key, item_json) VALUES (?1, ?2, ?3)",
            params![
                key,
                record.item.key.canonical(),
                encode("item_json", &item)?
            ],
        )?;
        transaction.execute(
            "INSERT INTO kanban_history_facts (collection, canonical_key, facts_json) VALUES (?1, ?2, ?3)",
            params![key, record.item.key.canonical(), encode("facts_json", &facts)?],
        )?;
    }
    Ok(())
}

fn read_samples(connection: &rusqlite::Connection) -> Result<Vec<Sample>> {
    let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'kanban_history_samples')",
            [], |row| row.get(0),
        )?;
    if !exists {
        return Ok(Vec::new());
    }
    let mut statement = connection.prepare(
        "SELECT collection, collection_json, coverage_json FROM kanban_history_samples ORDER BY collection",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut newest = HashMap::new();
    for row in rows {
        let (key, collection, coverage) = row?;
        let collection: Collection = decode("collection_json", &collection)?;
        collection.validate()?;
        let coverage: Coverage = decode("coverage_json", &coverage)?;
        let held = newest
            .entry(collection)
            .or_insert((key.clone(), coverage.clone()));
        if coverage.last_success >= held.1.last_success {
            *held = (key, coverage);
        }
    }
    let mut samples = Vec::new();
    for (collection, (key, coverage)) in newest {
        let mut records = connection.prepare(
                "SELECT h.item_json, f.facts_json FROM kanban_history h LEFT JOIN kanban_history_facts f \
                 ON h.collection = f.collection AND h.canonical_key = f.canonical_key \
                 WHERE h.collection = ?1 ORDER BY h.canonical_key"
            )?;
        let rows = records.query_map([key], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        let mut loaded = Vec::new();
        for row in rows {
            let (item, facts) = row?;
            let stored: StoredItem = decode("item_json", &item)?;
            if stored.version != 1 {
                continue;
            }
            let mut facts = facts
                .map(|json| decode::<StoredFacts>("facts_json", &json))
                .transpose()?
                .filter(|stored| stored.version == 1)
                .and_then(|stored| stored.facts)
                .unwrap_or_default();
            if facts.native_state.is_some() {
                facts.native_state = Some(NativeStateProvenance::CachedTracker);
            }
            loaded.push(Record {
                item: stored.item,
                facts,
            });
        }
        loaded.sort_by(|a, b| {
            b.item
                .updated_at
                .cmp(&a.item.updated_at)
                .then_with(|| a.item.key.canonical().cmp(&b.item.key.canonical()))
        });
        samples.push(Sample {
            collection,
            coverage,
            records: loaded,
        });
    }
    samples.sort_by_key(|sample| sample.collection.key());
    Ok(samples)
}

fn active_items(connection: &rusqlite::Connection) -> Result<Vec<Item>> {
    let mut statement =
        connection.prepare(&format!("SELECT {} FROM issues", super::ITEM_COLUMNS))?;
    let rows = statement.query_map([], |row| Ok(super::item_from_row(row)))?;
    rows.map(|row| row?).collect()
}
