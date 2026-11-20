use super::*;

fn restored(saved: &str) -> Store {
    Store::with_saved(read(saved.as_bytes()).unwrap(), None)
}

/// A file whose tabs have no title keeps every tab, each named after its
/// address as a page that gave no title is.
#[test]
fn a_saved_file_without_titles_keeps_its_tabs() {
    let store = restored(
        r#"{"tabs":[{"id":0,"scope":"local","workspace_id":"w","location":{"kind":"web","url":"https://a.test/docs"}},{"id":1,"scope":"local","workspace_id":"w","location":{"kind":"web","url":"https://b.test/"},"title":"B"},{"id":2,"scope":"local","workspace_id":"w","location":null}]}"#,
    );
    let labels: Vec<String> = (0..3)
        .map(|id| store.get(TabId(id)).unwrap().label())
        .collect();
    assert_eq!(labels[..2], ["a.test", "B"]);
    assert_eq!(store.get(TabId(0)).unwrap().title, "a.test");
    // A tab with no address has the app's name for a new tab.
    assert!(store.get(TabId(2)).unwrap().title.is_empty());
}
