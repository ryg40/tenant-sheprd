use super::*;
use crate::browser::WebUrl;

fn elsewhere() -> Page {
    Page::Elsewhere("other.example.test".into())
}

fn pages() -> Vec<Page> {
    vec![Page::Unseen, Page::OnWalkr, elsewhere()]
}

#[test]
fn classification_and_page_state_cover_the_connectivity_table() {
    for status in [200, 204, 299, 300, 302, 399] {
        assert_eq!(classify(Ok(status)), Reach::Answers);
    }
    for (reach, expected) in [
        (None, Connectivity::Checking),
        (Some(classify(Ok(403))), Connectivity::NetworkRefused),
        (Some(classify(Ok(502))), Connectivity::Down { status: 502 }),
        (
            Some(classify(Ok(404))),
            Connectivity::Unexpected { status: 404 },
        ),
    ] {
        for page in &pages() {
            assert_eq!(connectivity(true, reach, page), expected);
            assert_eq!(
                connectivity(false, reach, page),
                Connectivity::NotConfigured
            );
        }
    }
    for cause in [
        Unreachable::Dns,
        Unreachable::Connect,
        Unreachable::Timeout,
        Unreachable::Tls,
    ] {
        let reach = Some(classify(Err(cause)));
        for page in &pages() {
            assert_eq!(
                connectivity(true, reach, page),
                Connectivity::Unreachable { cause }
            );
        }
    }
    for (page, expected) in [
        (Page::Unseen, Connectivity::Reachable),
        (Page::OnWalkr, Connectivity::Ready),
        (elsewhere(), Connectivity::Reachable),
    ] {
        assert_eq!(connectivity(true, Some(Reach::Answers), &page), expected);
        assert_eq!(
            connectivity(false, Some(Reach::Answers), &page),
            Connectivity::NotConfigured
        );
    }
}

#[test]
fn a_changed_address_drops_the_old_result() {
    let config = WalkrConfig {
        url: Some(WebUrl::try_from("https://new.walkr.test").unwrap()),
        ..WalkrConfig::default()
    };
    let mut walkr = Walkr::default();
    let now = Instant::now();
    walkr.poll(false, &config, now);
    assert!(!walkr.apply(Update {
        generation: 0,
        reach: Reach::Answers,
        checked: now,
    }));
    assert_eq!(walkr.connectivity(), Connectivity::Checking);
    assert_eq!(walkr.age(&config, now), None);
}
