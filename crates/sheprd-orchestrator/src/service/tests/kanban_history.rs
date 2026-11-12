use super::*;
use crate::{
    Checkpoint, ItemKey,
    kanban::{
        facts::WorkflowFacts,
        history::{Collection, Origin, Read, Record, WINDOW},
    },
};

#[test]
fn newer_schema_stays_unwritten_with_live_history_and_independent_freshness() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("database/state.sqlite3");
    let now = Utc::now();
    let source = SourceKey {
        provider: Provider::Forgejo,
        host: "forgejo.example.test".into(),
        repository: "team/app".into(),
    };
    let mut active = Item::stand_in(ItemKey {
        source: source.clone(),
        native_id: "active".into(),
    });
    active.state = "open".into();
    active.updated_at = Some(now);
    let mut closed = active.clone();
    closed.key.native_id = "closed".into();
    closed.state = "closed".into();
    let checkpoint = Checkpoint {
        last_full_at: Some(now),
        ..Checkpoint::default()
    };
    let mut store = Store::open(&path).unwrap();
    store
        .replace_items(&source, std::slice::from_ref(&active), &checkpoint)
        .unwrap();
    let run = Run {
        id: "kept".into(),
        item_key: active.key.canonical(),
        workspace: None,
        agent: "pi".into(),
        model: None,
        state: crate::RunState::Running,
        message: None,
        session_id: None,
        started_at: now,
        updated_at: now,
        owner: crate::Owner::Sheprd,
    };
    store.save_run(&run, None).unwrap();
    drop(store);
    let db = rusqlite::Connection::open(&path).unwrap();
    let prepare = || {
        Service::prepare(Request {
            target: ConnectTarget::Local,
            checkout: String::new(),
            workspace_id: String::new(),
            token: None,
            azure_store: Default::default(),
            gitlab_store: Default::default(),
            data_root: Some(dir.path().to_owned()),
            timing: Timing::default(),
        })
    };
    let (_, mut worker, _) = prepare();
    worker.store = Some(Store::open(&path).unwrap());
    worker
        .keep_listing(
            &source,
            crate::forge::Listing {
                items: Vec::new(),
                checkpoint: Checkpoint::default(),
                complete: false,
            },
            true,
        )
        .unwrap();
    assert_eq!(
        worker.store.as_ref().unwrap().items().unwrap(),
        std::slice::from_ref(&active)
    );
    assert_eq!(
        worker.store.as_ref().unwrap().runs().unwrap(),
        std::slice::from_ref(&run)
    );
    // Local history writes and validation failures must not impose provider cooldowns.
    let collection = Collection::Repository(source.clone());
    let instant = Instant::now();
    let read = || Read {
        complete: true,
        cutoff: now - WINDOW,
        records: vec![Record {
            item: closed.clone(),
            facts: WorkflowFacts::default(),
        }],
        truncated: true,
    };
    worker.history.read(
        collection.clone(),
        kanban::ReadContext {
            active: &[],
            blocked_until: None,
            now: instant,
            observed_at: now,
            store: worker.store.as_mut(),
            visible: true,
        },
        |_| Ok(Some(read())),
    );
    db.execute_batch("CREATE TRIGGER reject_history BEFORE INSERT ON kanban_history BEGIN SELECT RAISE(ABORT, 'probe'); END;").unwrap();
    worker.history.refresh();
    worker.history.read(
        collection.clone(),
        kanban::ReadContext {
            active: &[],
            blocked_until: None,
            now: instant,
            observed_at: now,
            store: worker.store.as_mut(),
            visible: true,
        },
        |_| Ok(Some(read())),
    );
    worker.history.publish(&mut worker.snapshot, now);
    assert!(matches!(
        worker.snapshot.history[0].failure.as_deref(),
        Some(Error::Database(_))
    ));
    assert!(worker.history.provider_until(&collection).is_none());
    let active_state = worker
        .keep_listing(
            &source,
            crate::forge::Listing {
                items: vec![active.clone()],
                checkpoint: checkpoint.clone(),
                complete: true,
            },
            true,
        )
        .unwrap();
    assert!(matches!(active_state, SyncState::Synced { .. }));
    db.execute_batch("DROP TRIGGER reject_history").unwrap();

    // A live sample must accept another worker's same-success reopen removal.
    let mut reopened = closed.clone();
    reopened.state = "open".into();
    reopened.updated_at = Some(now + chrono::TimeDelta::seconds(1));
    let mut peer = Store::open(&path).unwrap();
    peer.reconcile_history(std::slice::from_ref(&reopened), now)
        .unwrap();
    worker.reload().unwrap();
    assert!(
        !worker
            .snapshot
            .board_records(now)
            .iter()
            .any(|record| record.item == closed)
    );
    assert_eq!(worker.snapshot.history[0].origin, Some(Origin::Live));

    // Repository and account workers persist capped reopen observations independently
    // of their complete active cache, and service recreation cannot revive Done.
    let seed = crate::kanban::history::Sample::from_read(collection.clone(), read(), now).unwrap();
    worker
        .store
        .as_mut()
        .unwrap()
        .replace_history(&seed, &[], now)
        .unwrap();
    worker
        .keep_listing(
            &source,
            crate::forge::Listing {
                items: vec![reopened.clone()],
                checkpoint: Checkpoint::default(),
                complete: false,
            },
            true,
        )
        .unwrap();
    worker.reload().unwrap();
    drop(worker);
    let (_, mut worker, _) = prepare();
    worker.store = Some(Store::open(&path).unwrap());
    worker
        .keep_listing(
            &source,
            crate::forge::Listing {
                items: Vec::new(),
                checkpoint: Checkpoint::default(),
                complete: false,
            },
            true,
        )
        .unwrap();
    worker.set_state(&source, SyncState::Failed(Arc::new(Error::Cancelled)));
    worker.reload().unwrap();
    assert!(
        !worker
            .snapshot
            .board_records(now)
            .iter()
            .any(|record| record.item == closed)
    );

    let account_root = dir.path().join("account");
    let account_request = || {
        let mut request = RemoteRequest::new(RemoteScope::ForgejoAccount {
            host: source.host.clone(),
        });
        request.look_up_token = false;
        request.data_root = Some(account_root.clone());
        request
    };
    let (account_service, mut account, _) =
        Service::prepare_forgejo_account(source.host.clone(), account_request());
    account.open();
    account
        .keep(crate::forge::Listing {
            items: vec![active.clone()],
            checkpoint: checkpoint.clone(),
            complete: true,
        })
        .unwrap();
    let account_path = database_path_in(
        &account_root,
        &crate::Repository::ForgejoAccount {
            host: source.host.clone(),
        },
    );
    let mut account_store = Store::open(&account_path).unwrap();
    account_store.save_run(&run, None).unwrap();
    let account_collection = Collection::ForgejoAccount {
        host: source.host.clone(),
    };
    let account_sample =
        crate::kanban::history::Sample::from_read(account_collection, read(), now).unwrap();
    account_store
        .replace_history(&account_sample, &[], now)
        .unwrap();
    account
        .keep(crate::forge::Listing {
            items: vec![reopened.clone()],
            checkpoint: Checkpoint::default(),
            complete: false,
        })
        .unwrap();
    drop(account);
    drop(account_service);
    let (account_service, mut account, _) =
        Service::prepare_forgejo_account(source.host.clone(), account_request());
    account.open();
    account
        .keep(crate::forge::Listing {
            items: Vec::new(),
            checkpoint: Checkpoint::default(),
            complete: false,
        })
        .unwrap();
    account.sync_history();
    assert!(
        !account_service
            .poll()
            .unwrap()
            .board_records(now)
            .iter()
            .any(|record| record.item == closed)
    );
    assert_eq!(account_store.items().unwrap(), [active.clone()]);
    assert_eq!(account_store.runs().unwrap(), std::slice::from_ref(&run));
    assert_eq!(
        account_store
            .checkpoint(&forgejo::search::account_source(&source.host))
            .unwrap(),
        Some(checkpoint.clone())
    );
    assert_eq!(
        worker.store.as_ref().unwrap().items().unwrap(),
        [active.clone()]
    );
    assert_eq!(
        worker.store.as_ref().unwrap().runs().unwrap(),
        std::slice::from_ref(&run)
    );
    assert_eq!(
        worker.store.as_ref().unwrap().checkpoint(&source).unwrap(),
        Some(checkpoint.clone())
    );

    // Keep cached closed evidence to test read-only suppression through later reloads.
    worker
        .store
        .as_mut()
        .unwrap()
        .replace_history(&seed, &[], now)
        .unwrap();
    worker.history = kanban::History::default();
    worker.store = None;
    worker.uncached.clear();
    db.pragma_update(None, "user_version", crate::SCHEMA_VERSION + 1)
        .unwrap();
    let version_before: i64 = db
        .query_row("PRAGMA data_version", [], |row| row.get(0))
        .unwrap();
    worker.store = Some(Store::open(&path).unwrap());
    worker.snapshot.sources = vec![SourceStatus {
        key: source.clone(),
        state: SyncState::Synced { at: now, count: 1 },
    }];
    worker.reload().unwrap();
    let apply = |worker: &mut Worker, at, outcome: Result<Option<Read>>| {
        worker.history.read(
            collection.clone(),
            kanban::ReadContext {
                active: &worker.snapshot.items,
                blocked_until: None,
                now: at,
                observed_at: now,
                store: worker.store.as_mut(),
                visible: true,
            },
            |_| outcome,
        );
        worker.history.publish(&mut worker.snapshot, now);
    };
    apply(&mut worker, instant, Ok(Some(read())));
    let successful = worker.snapshot.history[0].sample.clone().unwrap();
    assert_eq!(worker.snapshot.history[0].origin, Some(Origin::Live));
    assert_eq!(successful.coverage().last_success, now);
    assert!(successful.coverage().truncated);
    assert_eq!(successful.records()[0].item, closed);
    let board = worker.snapshot.board_records(now);
    assert!(board.iter().any(|record| record.item == active));
    assert!(board.iter().any(|record| record.item == closed));
    let expired = worker
        .snapshot
        .board_records(now + WINDOW + chrono::TimeDelta::seconds(1));
    assert_eq!(
        expired
            .iter()
            .map(|record| &record.item)
            .collect::<Vec<_>>(),
        [&active]
    );
    assert_eq!(
        worker.store.as_ref().unwrap().history().unwrap()[0].records()[0].item,
        closed
    );
    assert!(matches!(
        worker
            .store
            .as_mut()
            .unwrap()
            .replace_history(&successful, &[], now),
        Err(Error::NewerSchema { .. })
    ));
    assert!(matches!(
        worker.store.as_mut().unwrap().expire_history(now),
        Err(Error::NewerSchema { .. })
    ));
    // A capped active result can be displayed, but cannot replace stored active data or history.
    worker
        .keep_listing(
            &source,
            crate::forge::Listing {
                items: Vec::new(),
                checkpoint: Checkpoint::default(),
                complete: false,
            },
            false,
        )
        .unwrap();
    worker.set_state(&source, SyncState::Failed(Arc::new(Error::Cancelled)));
    worker.reload().unwrap();
    assert_eq!(
        worker.snapshot.history[0].sample.as_ref(),
        Some(&successful)
    );
    assert!(worker.snapshot.history[0].failure.is_none());
    // Incomplete and failed history reads keep both its previous sample and active failure.
    let mut incomplete = read();
    incomplete.complete = false;
    apply(
        &mut worker,
        instant + Duration::from_secs(3600),
        Ok(Some(incomplete)),
    );
    assert!(matches!(
        worker.snapshot.history[0].failure.as_deref(),
        Some(Error::Kanban(crate::kanban::Error::HistoryIncomplete))
    ));
    assert_eq!(
        worker.snapshot.history[0].sample.as_ref(),
        Some(&successful)
    );
    assert!(matches!(
        worker.snapshot.sources[0].state,
        SyncState::Failed(_)
    ));
    assert!(worker.history.provider_until(&collection).is_none());
    let due = instant + Duration::from_secs(7200);
    apply(
        &mut worker,
        due,
        Err(Error::Database(rusqlite::Error::InvalidQuery)),
    );
    assert!(matches!(
        worker.snapshot.history[0].failure.as_deref(),
        Some(Error::Database(_))
    ));
    assert_eq!(
        worker.snapshot.history[0].sample.as_ref(),
        Some(&successful)
    );
    assert!(worker.history.provider_until(&collection).is_none());
    let mut invalid = read();
    invalid.records[0].item.state = "open".into();
    apply(
        &mut worker,
        instant + Duration::from_secs(10800),
        Ok(Some(invalid)),
    );
    assert!(matches!(
        worker.snapshot.history[0].failure.as_deref(),
        Some(Error::Kanban(crate::kanban::Error::HistoryRecord))
    ));
    assert!(worker.history.provider_until(&collection).is_none());
    assert_eq!(
        worker.snapshot.history[0].sample.as_ref(),
        Some(&successful)
    );
    apply(
        &mut worker,
        instant + Duration::from_secs(14400),
        Ok(Some(read())),
    );

    // Case changes share in-memory history, without rewriting stored Item keys.
    let mut alias = source.clone();
    alias.host.make_ascii_uppercase();
    alias.repository.make_ascii_uppercase();
    worker.history.refresh();
    worker.history.read(
        Collection::Repository(alias),
        kanban::ReadContext {
            active: &[],
            blocked_until: None,
            now: instant + Duration::from_secs(18000),
            observed_at: now,
            store: worker.store.as_mut(),
            visible: true,
        },
        |_| {
            Ok(Some(Read {
                records: Vec::new(),
                ..read()
            }))
        },
    );
    worker.history.publish(&mut worker.snapshot, now);
    assert!(
        !worker
            .snapshot
            .board_records(now)
            .iter()
            .any(|record| record.item == closed)
    );
    worker.reload().unwrap();
    assert!(
        !worker
            .snapshot
            .board_records(now)
            .iter()
            .any(|record| record.item == closed)
    );
    apply(
        &mut worker,
        instant + Duration::from_secs(21600),
        Ok(Some(read())),
    );
    worker
        .keep_listing(
            &source,
            crate::forge::Listing {
                items: vec![reopened],
                checkpoint: Checkpoint::default(),
                complete: false,
            },
            false,
        )
        .unwrap();
    worker.reload().unwrap();
    worker
        .keep_listing(
            &source,
            crate::forge::Listing {
                items: Vec::new(),
                checkpoint: Checkpoint::default(),
                complete: false,
            },
            false,
        )
        .unwrap();
    worker.reload().unwrap();
    assert!(
        !worker
            .snapshot
            .board_records(now)
            .iter()
            .any(|record| record.item == closed)
    );
    assert_eq!(worker.snapshot.history[0].origin, Some(Origin::Live));
    assert_eq!(
        worker.snapshot.history[0]
            .sample
            .as_ref()
            .unwrap()
            .coverage()
            .last_success,
        now
    );
    assert_eq!(
        worker.store.as_ref().unwrap().history().unwrap()[0].records()[0].item,
        closed
    );
    assert_eq!(worker.store.as_ref().unwrap().items().unwrap(), [active]);
    assert_eq!(worker.store.as_ref().unwrap().runs().unwrap(), [run]);
    assert_eq!(
        worker.store.as_ref().unwrap().checkpoint(&source).unwrap(),
        Some(checkpoint)
    );
    let version_after: i64 = db
        .query_row("PRAGMA data_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version_before, version_after);
}
