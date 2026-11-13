//! The account's listing is replaced only with a complete one: a capped
//! listing is shown and not written, so a restart shows the last complete
//! listing, each issue under its own repository.
use super::*;
use crate::{Checkpoint, Item, ItemKey, Provider, RemoteScope, SourceKey};

mod kanban_history;
mod kanban_moves;

const HOST: &str = "forgejo.example.test";

fn issue(repository: &str, number: u64) -> Item {
    let mut item = Item::stand_in(ItemKey {
        source: SourceKey {
            provider: Provider::Forgejo,
            host: HOST.into(),
            repository: repository.into(),
        },
        native_id: number.to_string(),
    });
    item.identifier = format!("#{number}");
    item.title = format!("Issue {number} of {repository}");
    item.state = "open".into();
    item
}

fn listing(items: Vec<Item>, complete: bool) -> Listing {
    Listing {
        items,
        checkpoint: Checkpoint::default(),
        complete,
    }
}

fn worker(root: &std::path::Path) -> AccountWorker {
    let mut request = RemoteRequest::new(RemoteScope::ForgejoAccount { host: HOST.into() });
    request.look_up_token = false;
    request.data_root = Some(root.to_owned());
    let (_service, mut worker, _commands) = Service::prepare_forgejo_account(HOST.into(), request);
    worker.open();
    worker
}

fn keys(items: &[Item]) -> Vec<String> {
    let mut keys: Vec<String> = items.iter().map(|item| item.key.canonical()).collect();
    keys.sort();
    keys
}

#[test]
fn only_a_complete_listing_is_stored_and_a_restart_shows_it() {
    let dir = tempfile::tempdir().unwrap();
    let complete = vec![issue("team/app", 1), issue("team/web", 1)];
    let mut first = worker(dir.path());
    assert!(first.snapshot.items.is_empty(), "nothing stored yet");
    let state = first.keep(listing(complete.clone(), true)).unwrap();
    assert!(matches!(state, SyncState::Synced { .. }), "{state:?}");
    assert_eq!(keys(&first.snapshot.items), keys(&complete));

    // A capped listing is shown, and the database keeps the complete one.
    let capped = vec![issue("team/api", 7)];
    let state = first.keep(listing(capped.clone(), false)).unwrap();
    assert!(matches!(state, SyncState::Capped { .. }), "{state:?}");
    assert_eq!(keys(&first.snapshot.items), keys(&capped));
    assert!(first.snapshot.access.is_none());
    drop(first);

    let restarted = worker(dir.path());
    assert_eq!(keys(&restarted.snapshot.items), keys(&complete));
    // Each stored issue keeps its own repository, not the account's source.
    let repositories: Vec<&str> = restarted
        .snapshot
        .items
        .iter()
        .map(|item| item.key.source.repository.as_str())
        .collect();
    assert!(repositories.contains(&"team/app") && repositories.contains(&"team/web"));
    let path = database_path_in(
        dir.path(),
        &Repository::ForgejoAccount { host: HOST.into() },
    );
    let stored = Store::open(&path).unwrap();
    assert!(
        stored
            .checkpoint(&forgejo::search::account_source(HOST))
            .unwrap()
            .is_some(),
        "the listing is stored under the account's source"
    );
}

#[test]
fn a_complete_listing_replaces_issues_closed_since() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = worker(dir.path());
    first
        .keep(listing(
            vec![issue("team/app", 1), issue("team/web", 2)],
            true,
        ))
        .unwrap();
    first
        .keep(listing(vec![issue("team/web", 2)], true))
        .unwrap();
    drop(first);
    let restarted = worker(dir.path());
    assert_eq!(
        keys(&restarted.snapshot.items),
        keys(&[issue("team/web", 2)])
    );
}
