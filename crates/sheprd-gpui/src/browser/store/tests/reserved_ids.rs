use super::*;

#[test]
fn restoring_reserved_ids_keeps_other_tabs_and_never_allocates_a_panel_id() {
    let mut original = Store::default();
    let scope = Scope::endpoint("local");
    let kept = original
        .open(
            scope.clone(),
            "w_1",
            url("https://walkr.example.test"),
            None,
        )
        .unwrap();
    let tab = original.get(kept).unwrap().clone();
    for reserved in [TabId::WALKR, TabId::panel(1), TabId(u64::MAX / 2 + 1)] {
        let mut saved = original.saved();
        saved.tabs.push(Tab {
            id: reserved,
            ..tab.clone()
        });
        let bytes = serde_json::to_vec(&saved).unwrap();
        let mut restored = Store::with_saved(read(&bytes).unwrap(), None);
        assert_eq!(restored.tabs().collect::<Vec<_>>(), vec![&tab]);
        let opened = restored.open(scope.clone(), "w_1", None, None).unwrap();
        assert!(!opened.is_panel());
        assert_ne!(opened, kept);
    }

    let mut saved = original.saved();
    saved.tabs.push(Tab {
        id: TabId(u64::MAX / 2 - 1),
        ..tab.clone()
    });
    let mut restored = Store::with_saved(read(&serde_json::to_vec(&saved).unwrap()).unwrap(), None);
    let last = restored.open(scope.clone(), "w_1", None, None).unwrap();
    assert!(!last.is_panel());
    assert!(restored.open(scope, "w_1", None, None).is_none());
    assert_eq!(restored.get(kept), Some(&tab));
}
