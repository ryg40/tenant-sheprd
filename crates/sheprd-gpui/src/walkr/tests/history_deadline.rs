#![allow(clippy::unwrap_used)]
use super::*;

#[test]
fn an_answer_finished_after_its_deadline_is_not_shown() {
    let deadline = Instant::now();
    let finished = deadline + Duration::from_millis(1);
    let id = "20261101T120000Z-0123456789ab";
    let record = format!(r#"{{"schema":"walkr-compare-session","version":1,"sessionId":"{id}"}}"#);
    let detail = history::parse(id, record.as_bytes(), history::RATINGS_PAGE).unwrap();
    let mut sessions = Sessions {
        pick: Some(id.into()),
        detail: Some((id.into(), Shown::Loading)),
        detail_waiting: Some(deadline),
        list_waiting: Some(deadline),
        ..Sessions::default()
    };
    sessions.apply(
        Answer::Detail {
            generation: 0,
            finished,
            result: Ok(Box::new(detail)),
        },
        finished,
    );
    assert!(matches!(
        sessions.detail(),
        Some(Shown::Failed(history::Error::TimedOut))
    ));
    sessions.apply(
        Answer::List {
            generation: 0,
            finished,
            rows: vec![history::row(id, record.as_bytes())],
        },
        finished,
    );
    assert!(sessions.rows().is_empty());
    assert!(!sessions.listed_since(deadline));
    assert!(!sessions.fresh(finished));
    assert!(sessions.failed_since(deadline));
}
