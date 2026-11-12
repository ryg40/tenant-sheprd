use super::*;
use crate::browser::WebUrl;

#[test]
fn page_loaded_classifies_the_origin_and_keeps_probe_failures() {
    let configured = WebUrl::try_from("https://walkr.example.test/app").unwrap();
    let config = WalkrConfig {
        url: Some(configured.clone()),
        ..WalkrConfig::default()
    };
    let mut walkr = Walkr::default();
    for (loaded, page, state) in [
        (
            "https://walkr.example.test:443/review",
            Page::OnWalkr,
            Connectivity::Ready,
        ),
        (
            "https://other.example.test/flow?client_id=synthetic",
            Page::Elsewhere("other.example.test".into()),
            Connectivity::Reachable,
        ),
        (
            "https://walkr.example.test:8443/",
            Page::Elsewhere("walkr.example.test".into()),
            Connectivity::Reachable,
        ),
        (
            "http://walkr.example.test/",
            Page::Elsewhere("walkr.example.test".into()),
            Connectivity::Reachable,
        ),
    ] {
        walkr.test_reach(&config, Some(Reach::Answers));
        walkr.page_loaded(loaded, &configured);
        assert_eq!(walkr.page, page);
        assert_eq!(walkr.connectivity(), state);
        for invalid in ["about:blank", "garbage"] {
            walkr.page_loaded(invalid, &configured);
            assert_eq!(walkr.page, page);
        }
        for reach in [
            None,
            Some(Reach::NetworkRefused),
            Some(Reach::Down(502)),
            Some(Reach::Unexpected(404)),
            Some(Reach::Unreachable(Unreachable::Connect)),
        ] {
            walkr.test_reach(&config, reach);
            assert_eq!(
                walkr.connectivity(),
                connectivity(true, reach, &Page::Unseen)
            );
        }
    }
}

#[test]
fn a_page_on_another_origin_is_named_by_its_host_alone() {
    let configured = WebUrl::try_from("http://127.0.0.1:4100").unwrap();
    let config = WalkrConfig {
        url: Some(configured.clone()),
        ..WalkrConfig::default()
    };
    let mut walkr = Walkr::default();
    walkr.test_reach(&config, Some(Reach::Answers));
    walkr.page_loaded(
        "http://walkr.example.test:8443/_walkr/compare/g/INVENTEDSECRET?code=synthetic#20261101T120000Z-0123456789ab",
        &configured,
    );
    assert_eq!(walkr.connectivity(), Connectivity::Reachable);
    let shown = crate::walkr::section::title(&walkr.label(), "127.0.0.1:4100");
    assert!(shown.contains("walkr.example.test"), "{shown}");
    for hidden in ["INVENTEDSECRET", "_walkr", "synthetic", "8443", "20261101"] {
        assert!(!shown.contains(hidden), "{shown}");
    }
    // A failed probe has priority over the page, in the header too.
    walkr.test_reach(&config, Some(Reach::Down(502)));
    assert_eq!(
        walkr.label(),
        header::Label::State(Connectivity::Down { status: 502 })
    );
}
