#![allow(clippy::unwrap_used)]

use super::*;
use crate::{Access, Checkpoint, HerdrSession, Owner, Run, RunState, Store};
use chrono::{TimeDelta, Utc};
use history::{Collection, Read, Record, Sample, WINDOW};

fn record(source: &SourceKey, id: &str, at: chrono::DateTime<Utc>) -> Record {
    let mut item = issue(source, id, "closed", &[]);
    item.updated_at = Some(at);
    Record {
        item,
        facts: complete(),
    }
}

fn sample(
    source: &SourceKey,
    records: Vec<Record>,
    now: chrono::DateTime<Utc>,
    truncated: bool,
) -> Sample {
    Sample::from_read(
        Collection::Repository(source.clone()),
        Read {
            complete: true,
            cutoff: now - WINDOW,
            records,
            truncated,
        },
        now,
    )
    .unwrap()
}

fn run(item: &Item, now: chrono::DateTime<Utc>) -> Run {
    Run {
        id: "history-run".into(),
        item_key: item.key.canonical(),
        workspace: None,
        agent: "pi".into(),
        model: None,
        state: RunState::Running,
        message: None,
        session_id: None,
        started_at: now,
        updated_at: now,
        owner: Owner::Sheprd,
    }
}

#[test]
fn migration_and_history_refresh_preserve_active_rows_and_runs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("database/state.sqlite3");
    let now = Utc::now();
    let source = source(Provider::Forgejo, "forgejo.example.test", "team/app");
    let active = issue(&source, "active", "open", &[]);
    let run = run(&active, now);
    let session = HerdrSession {
        run_id: run.id.clone(),
        host: None,
        session: None,
        workspace_id: "w1".into(),
        pane_id: "p1".into(),
        agent_name: "worker".into(),
        updated_at: now,
    };
    let checkpoint = Checkpoint {
        last_full_at: Some(now),
        ..Checkpoint::default()
    };
    let mut store = Store::open(&path).unwrap();
    store
        .replace_items(&source, std::slice::from_ref(&active), &checkpoint)
        .unwrap();
    store.save_run(&run, Some(&session)).unwrap();
    drop(store);
    let db = rusqlite::Connection::open(&path).unwrap();
    // Simulate the schema shipped before history, with existing data in every protected table.
    db.execute_batch("DROP TABLE kanban_history; DROP TABLE kanban_history_facts; DROP TABLE kanban_history_samples; PRAGMA user_version = 3;").unwrap();
    db.execute(
        "INSERT INTO events (run_id, sequence, timestamp, payload_json) VALUES (?1, 0, ?2, '{}')",
        rusqlite::params![run.id, now.to_rfc3339()],
    )
    .unwrap();
    db.execute(
        "UPDATE runs SET owner = 'agent-launcher' WHERE id = ?1",
        [&run.id],
    )
    .unwrap();
    let mut owned_elsewhere = run.clone();
    owned_elsewhere.owner = Owner::AgentLauncher;
    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.access(), Access::ReadWrite);
    assert_eq!(store.items().unwrap(), std::slice::from_ref(&active));
    assert_eq!(store.checkpoint(&source).unwrap(), Some(checkpoint.clone()));
    assert_eq!(
        store.runs().unwrap(),
        std::slice::from_ref(&owned_elsewhere)
    );
    assert_eq!(store.sessions().unwrap(), std::slice::from_ref(&session));

    let first = sample(&source, vec![record(&source, "old", now)], now, false);
    store.replace_history(&first, &[], now).unwrap();
    let candidates = vec![record(&source, "closed", now)];
    let capped = sample(&source, candidates.clone(), now, true);
    store.replace_history(&capped, &[], now).unwrap();
    let kept = store.history().unwrap();
    assert!(kept[0].coverage().truncated);
    assert_eq!(
        kept[0]
            .records()
            .iter()
            .map(|record| &record.item)
            .collect::<Vec<_>>(),
        candidates
            .iter()
            .map(|record| &record.item)
            .collect::<Vec<_>>()
    );

    // Case aliases replace the same sample, while item spelling stays untouched.
    for provider in [Provider::Github, Provider::Forgejo] {
        let host = match provider {
            Provider::Github => "github.example.test",
            _ => "forgejo.example.test",
        };
        let lower = super::source(provider, host, "team/aliases");
        let upper = super::source(provider, &host.to_ascii_uppercase(), "TEAM/ALIASES");
        let latest = record(&upper, "latest", now);
        store
            .replace_history(
                &sample(&lower, vec![record(&lower, "older", now)], now, false),
                &[],
                now,
            )
            .unwrap();
        store
            .replace_history(
                &sample(
                    &upper,
                    vec![latest.clone()],
                    now + TimeDelta::seconds(1),
                    false,
                ),
                &[],
                now,
            )
            .unwrap();
        assert_eq!(
            store
                .history()
                .unwrap()
                .iter()
                .filter(|sample| sample.collection() == &Collection::Repository(lower.clone()))
                .flat_map(Sample::records)
                .map(|record| &record.item)
                .collect::<Vec<_>>(),
            [&latest.item]
        );
        // Simulate pre-fix alias tables, whose keys and collection JSON kept case.
        let legacy = Collection::Repository(upper.clone());
        for table in [
            "kanban_history",
            "kanban_history_facts",
            "kanban_history_samples",
        ] {
            db.execute(
                &format!("UPDATE {table} SET collection = ?1 WHERE collection = ?2"),
                rusqlite::params![upper.canonical(), lower.canonical()],
            )
            .unwrap();
        }
        let old_sample = sample(&lower, vec![record(&lower, "obsolete", now)], now, false);
        db.execute(
            "INSERT INTO kanban_history_samples VALUES (?1, ?2, ?3)",
            rusqlite::params![
                lower.canonical(),
                serde_json::to_string(&old_sample.collection).unwrap(),
                serde_json::to_string(old_sample.coverage()).unwrap()
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO kanban_history VALUES (?1, ?2, ?3)",
            rusqlite::params![
                lower.canonical(),
                old_sample.records()[0].item.key.canonical(),
                serde_json::json!({"version": 1, "item": old_sample.records()[0].item}).to_string()
            ],
        )
        .unwrap();
        let loaded = store.history().unwrap();
        let actual = loaded
            .iter()
            .find(|sample| sample.collection() == &legacy)
            .unwrap();
        assert_eq!(
            actual
                .records()
                .iter()
                .map(|record| &record.item)
                .collect::<Vec<_>>(),
            [&latest.item]
        );
        let keys: Vec<String> = db
            .prepare("SELECT collection FROM kanban_history_samples WHERE collection IN (?1, ?2)")
            .unwrap()
            .query_map(
                rusqlite::params![lower.canonical(), upper.canonical()],
                |row| row.get(0),
            )
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(keys, [lower.canonical()]);
    }
    let account = Collection::ForgejoAccount {
        host: "FORGEJO.EXAMPLE.TEST".into(),
    };
    let lower_account = Collection::ForgejoAccount {
        host: "forgejo.example.test".into(),
    };
    let account_sample = Sample::from_read(
        account,
        Read {
            complete: true,
            cutoff: now - WINDOW,
            records: vec![record(&source, "account-old", now)],
            truncated: false,
        },
        now,
    )
    .unwrap();
    store.replace_history(&account_sample, &[], now).unwrap();
    let account_new = record(&source, "account-new", now);
    let account_sample = Sample::from_read(
        lower_account.clone(),
        Read {
            complete: true,
            cutoff: now - WINDOW,
            records: vec![account_new.clone()],
            truncated: true,
        },
        now + TimeDelta::seconds(1),
    )
    .unwrap();
    store.replace_history(&account_sample, &[], now).unwrap();
    let kept = store.history().unwrap();
    assert_eq!(
        kept.iter()
            .filter(|sample| sample.collection() == &lower_account)
            .flat_map(Sample::records)
            .map(|record| &record.item)
            .collect::<Vec<_>>(),
        [&account_new.item]
    );

    assert!(matches!(
        Sample::from_read(
            Collection::Repository(source.clone()),
            Read {
                complete: false,
                cutoff: now - WINDOW,
                records: Vec::new(),
                truncated: true,
            },
            now
        ),
        Err(Error::HistoryIncomplete)
    ));
    // A cache write that fails after deleting old rows must roll back the whole sample.
    db.execute_batch("CREATE TRIGGER reject_history BEFORE INSERT ON kanban_history BEGIN SELECT RAISE(ABORT, 'probe'); END;").unwrap();
    assert!(matches!(
        store.replace_history(&first, &[], now),
        Err(crate::Error::Database(_))
    ));
    assert_eq!(store.history().unwrap(), kept);
    assert_eq!(store.items().unwrap(), [active]);
    assert_eq!(store.checkpoint(&source).unwrap(), Some(checkpoint));
    assert_eq!(store.runs().unwrap(), [owned_elsewhere]);
    assert_eq!(store.sessions().unwrap(), [session]);
    let event: String = db
        .query_row(
            "SELECT payload_json FROM events WHERE run_id = ?1",
            [&run.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&event).unwrap(),
        serde_json::json!({})
    );
}

#[test]
fn reopen_and_expiry_remove_only_history_not_items_or_runs() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("database/state.sqlite3")).unwrap();
    let now = Utc::now();
    let source = source(Provider::Forgejo, "forgejo.example.test", "team/app");
    let old = record(&source, "reopened", now - TimeDelta::days(2));
    let expires = record(&source, "expires", now - WINDOW + TimeDelta::hours(1));
    let foreign = super::source(Provider::Github, "github.example.test", "team/app");
    let other = record(&foreign, "reopened", now);
    let mut open = old.item.clone();
    open.state = "open".into();
    open.updated_at = Some(now);
    open.key.source.repository = "TEAM/APP".into();
    let run = run(&open, now);
    store.save_run(&run, None).unwrap();
    store
        .replace_history(
            &sample(&source, vec![old.clone(), expires], now, false),
            &[],
            now,
        )
        .unwrap();
    store
        .replace_history(&sample(&foreign, vec![other.clone()], now, false), &[], now)
        .unwrap();
    // An older open observation cannot erase newer settlement evidence.
    let mut stale = open.clone();
    stale.updated_at = Some(now - TimeDelta::days(3));
    store
        .replace_items(&source, &[stale], &Checkpoint::default())
        .unwrap();
    assert!(
        store
            .history()
            .unwrap()
            .iter()
            .flat_map(Sample::records)
            .any(|record| record.item == old.item)
    );
    // A capped page can prove a reopen without replacing the last complete listing.
    let active_before = store.items().unwrap();
    let checkpoint_before = store.checkpoint(&source).unwrap();
    store
        .reconcile_history(std::slice::from_ref(&open), now)
        .unwrap();
    let path = dir.path().join("database/state.sqlite3");
    drop(store);
    let mut store = Store::open(&path).unwrap();
    // The next capped page omits the reopened item; persisted removal must survive.
    store
        .reconcile_history(&[], now + TimeDelta::hours(2))
        .unwrap();
    let history = store.history().unwrap();
    assert_eq!(
        history
            .iter()
            .flat_map(Sample::records)
            .map(|record| &record.item)
            .collect::<Vec<_>>(),
        [&other.item]
    );
    assert_eq!(store.items().unwrap(), active_before);
    assert_eq!(store.checkpoint(&source).unwrap(), checkpoint_before);
    assert_eq!(store.runs().unwrap(), [run]);
    // An empty active listing is not native closure evidence.
    store
        .replace_items(&source, &[], &Checkpoint::default())
        .unwrap();
    assert_eq!(store.history().unwrap(), history);
}
