#![allow(clippy::unwrap_used)]
use super::*;

/// The path part of an address `replace` returned.
fn path(url: &str) -> String {
    let url = url.split('#').next().unwrap();
    url.strip_prefix(ORIGINS[0]).unwrap().to_owned()
}

/// A slot answers only its current document, at its own path, to a
/// request the app made; never another slot's bytes.
#[test]
fn a_document_slot_answers_only_its_current_document() {
    let (first, second) = (Document::default(), Document::default());
    assert_eq!(
        first.answer("/.sheprd/walkr-session-1.html", false).status,
        404
    );

    let retired = path(&first.replace(1, "FIRST-OLD".into(), None));
    let current = path(&first.replace(2, "FIRST".into(), Some("rating-3")));
    let other = path(&second.replace(2, "SECOND".into(), None));

    let answer = first.answer(&current, false);
    assert_eq!(answer.status, 200);
    assert_eq!(answer.body, b"FIRST");
    assert_eq!(second.answer(&other, false).body, b"SECOND");

    for refused in [
        retired.clone(),
        format!("{current}?session=other"),
        format!("{current}/"),
        "/".to_owned(),
        "/.sheprd/".to_owned(),
    ] {
        let answer = first.answer(&refused, false);
        assert_eq!(answer.status, 404, "{refused}");
        assert!(answer.body.is_empty(), "{refused}");
    }
    let answer = first.answer(&current, true);
    assert_eq!(answer.status, 403);
    assert!(answer.body.is_empty());
}
