//! A Forgejo account's stored listing reads as a repository's does: every
//! issue under its own repository, and nothing created when it never synced.
use super::super::items::read;
use crate::{
    cli::control::Items,
    orchestrator::{Checkpoint, Item, ItemKey, Provider, SourceKey, Store, forgejo},
};

fn issue(repository: &str, number: u64) -> Item {
    let mut item = Item::stand_in(ItemKey {
        source: SourceKey {
            provider: Provider::Forgejo,
            host: "forgejo.example.com".into(),
            repository: repository.into(),
        },
        native_id: number.to_string(),
    });
    item.identifier = format!("#{number}");
    item.title = format!("Issue {number} of {repository}");
    item.state = "open".into();
    item
}

#[test]
fn the_accounts_listing_is_read_from_its_database() {
    let dir = tempfile::tempdir().unwrap();
    // The store makes its own folder private, as the state directory is.
    let path = dir.path().join("account").join("state.sqlite3");
    let missing = read(&path, Items::Issues, "not yet", "this Forgejo account").unwrap();
    assert_eq!(missing.status, crate::control::client::exit::NOTHING);
    assert!(!path.exists(), "listing never creates a database");

    let mut store = Store::open(&path).unwrap();
    let source = forgejo::search::account_source("forgejo.example.com");
    store
        .replace_items(
            &source,
            &[issue("team/app", 1), issue("team/web", 1)],
            &Checkpoint::default(),
        )
        .unwrap();
    drop(store);
    let output = read(&path, Items::Issues, "not yet", "this Forgejo account").unwrap();
    assert_eq!(output.status, 0);
    // Each issue under its own repository, though both are number 1.
    let lines: Vec<&str> = output.text.lines().collect();
    for repository in ["team/app", "team/web"] {
        assert!(
            lines.contains(&format!("#1\topen\tIssue 1 of {repository}").as_str()),
            "{lines:?}"
        );
    }
}
