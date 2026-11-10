use super::*;

#[test]
fn a_page_without_a_title_names_its_tab_after_its_address() {
    let mut store = Store::default();
    let scope = Scope::endpoint("local");
    let id = store
        .open(scope.clone(), "w_1", url("https://a.test/"), None)
        .unwrap();
    assert!(store.visited(id, url("https://b.test/x"), Some("Docs")));
    assert!(store.untitled(id));
    assert_eq!(store.get(id).unwrap().title, "b.test");
    // The same answer again writes nothing.
    assert!(!store.untitled(id));

    // A host can be longer than a saved title may be. The file of a title
    // that is too long is refused whole at the next start.
    let label = "a".repeat(60);
    let host = format!("{label}.{label}.{label}.{label}.test");
    assert!(host.len() > MAX_TITLE_CHARS);
    assert!(store.visited(id, url(&format!("https://{host}/cb")), None));
    assert!(store.untitled(id));
    let title = store.get(id).unwrap().title.clone();
    assert!(host.starts_with(&title) && !title.is_empty(), "{title}");
    let bytes = serde_json::to_vec(&Saved {
        tabs: store.tabs.clone(),
        asked: Default::default(),
    })
    .unwrap();
    assert_eq!(parse(&bytes).unwrap(), store.tabs);

    let blank = store.open(scope.clone(), "w_1", None, None).unwrap();
    assert!(!store.untitled(blank));
    for editor in [Editor::Px0, Editor::VsCode] {
        let id = store
            .open_editor_tab(scope.clone(), "w_1", editor, None)
            .unwrap();
        store.visited(id, url("http://localhost:1234/"), None);
        assert!(!store.untitled(id));
        assert_eq!(store.get(id).unwrap().title, editor.name());
    }
}
