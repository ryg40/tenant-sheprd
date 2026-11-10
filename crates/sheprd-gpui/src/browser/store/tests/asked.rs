use super::*;

const SITE: &str = "https://site.test/";
const SIGN_IN: &str = "https://auth.test/flow?next=x";

fn saved(store: &Store) -> Vec<u8> {
    serde_json::to_vec(&store.saved()).unwrap()
}

fn restored(bytes: &[u8]) -> Store {
    Store::with_saved(read(bytes).unwrap(), None)
}

fn address(store: &Store, id: TabId) -> String {
    match &store.get(id).unwrap().location {
        Some(Location::Web { url }) => url.as_str().to_owned(),
        other => panic!("not a web address: {other:?}"),
    }
}

/// How many asked addresses `asked` holds.
fn count(asked: &crate::browser::store::asked::Asked) -> usize {
    serde_json::to_value(asked)
        .unwrap()
        .as_array()
        .unwrap()
        .len()
}

#[test]
fn a_tab_a_site_sent_to_a_sign_in_restarts_on_the_address_asked_for() {
    let mut store = Store::default();
    let scope = Scope::endpoint("local");
    let id = store.open(scope.clone(), "w_1", url(SITE), None).unwrap();
    // The site answers with its sign-in on another origin, before it
    // showed a page of its own. A reload there changes nothing.
    assert!(store.visited(id, url(SIGN_IN), Some("Sign in")));
    store.visited(id, url(SIGN_IN), None);
    let mut again = restored(&saved(&store));
    assert_eq!(address(&again, id), SITE);
    assert_eq!(again.get(id).unwrap().title, "site.test");
    // The restored tab is sent to the sign-in again; the next start still
    // begins at the site.
    assert!(again.visited(id, url(SIGN_IN), None));
    assert_eq!(address(&restored(&saved(&again)), id), SITE);
    // Signed in: the site's page loads, and the tab is like any other.
    again.visited(id, url("https://site.test/home"), None);
    assert_eq!(count(&again.saved().asked), 0);

    // An asked address is kept without its query and fragment, which may
    // hold a token: the file has no second copy of it.
    let token = store
        .open(
            scope,
            "w_1",
            url("https://site.test/app?token=s3cret#at"),
            None,
        )
        .unwrap();
    store.visited(token, url(SIGN_IN), None);
    let file = saved(&store);
    assert!(!String::from_utf8(file.clone()).unwrap().contains("s3cret"));
    assert_eq!(address(&restored(&file), token), "https://site.test/app");
}

/// The defect this guards: one click on a link to another site made the
/// tab restore on the page before the click.
#[test]
fn a_link_the_user_followed_to_another_site_restores_that_site() {
    let mut store = Store::default();
    let scope = Scope::endpoint("local");
    let id = store
        .open(scope, "w_1", url("https://news.test/"), None)
        .unwrap();
    // The asked page loads where it was asked for, then the user follows
    // one link to another origin.
    assert!(!store.visited(id, url("https://news.test/"), None));
    store.visited(id, url("https://article.test/story"), None);
    let again = restored(&saved(&store));
    assert_eq!(address(&again, id), "https://article.test/story");
}

#[test]
fn a_tab_the_user_browsed_on_restores_its_last_address() {
    let mut store = Store::default();
    let scope = Scope::endpoint("local");
    // On the asked site, however deep: the last address.
    let stayed = store.open(scope.clone(), "w_1", url(SITE), None).unwrap();
    store.visited(stayed, url("https://site.test/docs/deep"), None);
    // Sent to another host name, then on from there: the user reads that
    // site, and the last address is theirs.
    let moved = store
        .open(scope.clone(), "w_1", url("https://example.test/"), None)
        .unwrap();
    store.visited(moved, url("https://www.example.test/"), None);
    store.visited(moved, url("https://www.example.test/article"), None);
    // Signed in and back on the site, then on to another site by a link.
    let linked = store.open(scope.clone(), "w_1", url(SITE), None).unwrap();
    store.visited(linked, url(SIGN_IN), None);
    store.visited(linked, url("https://site.test/home"), None);
    store.visited(linked, url("https://other.test/a"), None);
    // A new address typed over a sign-in page is the one asked for.
    let typed = store.open(scope, "w_1", url(SITE), None).unwrap();
    store.visited(typed, url(SIGN_IN), None);
    assert!(store.asked(typed, url("https://third.test/x").unwrap()));
    store.visited(typed, url("https://third.test/y"), None);

    let again = restored(&saved(&store));
    assert_eq!(address(&again, stayed), "https://site.test/docs/deep");
    assert_eq!(address(&again, moved), "https://www.example.test/article");
    assert_eq!(address(&again, linked), "https://other.test/a");
    assert_eq!(address(&again, typed), "https://third.test/y");
}

#[test]
fn a_file_saved_before_asked_addresses_loads_as_it_was() {
    let mut store = Store::default();
    let scope = Scope::endpoint("local");
    let id = store.open(scope, "w_1", url(SITE), None).unwrap();
    store.visited(id, url(SIGN_IN), Some("Sign in"));
    // What an earlier build wrote: the tabs alone.
    let old = serde_json::to_vec(&serde_json::json!({ "tabs": store.tabs })).unwrap();
    let again = restored(&old);
    assert_eq!(again.tabs, store.tabs);
    assert_eq!(address(&again, id), SIGN_IN);
    // And what this build writes for such tabs is what the earlier build
    // wrote: a store without an asked address adds no field.
    assert_eq!(saved(&again), old);
    // An earlier build reads this build's file: it ignores the new field.
    #[derive(serde::Deserialize)]
    struct Earlier {
        tabs: Vec<Tab>,
    }
    let earlier: Earlier = serde_json::from_slice(&saved(&store)).unwrap();
    assert_eq!(earlier.tabs, store.tabs);
}

#[test]
fn asked_addresses_follow_their_tabs_and_stay_bounded() {
    let mut store = Store::default();
    let scope = Scope::endpoint("local");
    let id = store.open(scope.clone(), "w_1", url(SITE), None).unwrap();
    store.visited(id, url(SIGN_IN), None);
    // An editor tab and a tab without a web address keep none.
    let editor = store
        .open_editor_tab(scope.clone(), "w_1", Editor::VsCode, url(SITE))
        .unwrap();
    store.visited(editor, url("https://code.test/"), None);
    let blank = store.open(scope.clone(), "w_1", None, None).unwrap();
    assert_eq!(count(&store.saved().asked), 1);
    let file = saved(&store);
    assert_eq!(address(&restored(&file), editor), "https://code.test/");
    assert!(restored(&file).get(blank).unwrap().location.is_none());
    // A closed tab's address is not saved, and is dropped from memory when
    // the next tab is asked for an address: the list follows the tabs.
    store.close(id);
    assert_eq!(count(&store.saved().asked), 0);
    let next = store.open(scope, "w_1", url(SITE), None).unwrap();
    store.visited(next, url(SIGN_IN), None);
    assert_eq!(count(&store.asked), 1);
    // More entries than the app keeps tabs, or an address that is no web
    // address, refuse the file whole.
    let entry = r#"{"tab":0,"url":"https://a.test/"}"#;
    let many = vec![entry; MAX_TABS + 1].join(",");
    for asked in [many.as_str(), r#"{"tab":0,"url":"file:///etc/passwd"}"#] {
        let file = format!(r#"{{"tabs":[],"asked":[{asked}]}}"#);
        assert!(read(file.as_bytes()).is_err());
    }
}
