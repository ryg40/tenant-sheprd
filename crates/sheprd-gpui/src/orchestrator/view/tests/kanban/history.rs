use super::*;
use sheprd_orchestrator::{
    Store,
    kanban::{
        facts::{NativeStateProvenance, WorkflowFacts},
        history::{Collection, Origin, Read, Record, Sample, Status, WINDOW},
    },
};

#[test]
fn history_deduplicates_open_observations_and_failed_reads_keep_the_last_sample() {
    let now = Utc::now();
    let mut open = fixture("forgejo.example.test", "Reopened issue");
    open.key.source.provider = Provider::Forgejo;
    open.updated_at = Some(now);
    let mut closed = open.clone();
    closed.state = "closed".into();
    closed.updated_at = Some(now - chrono::TimeDelta::hours(1));
    let mut settled = closed.clone();
    settled.key.native_id = "8".into();
    let collection = Collection::Repository(open.key.source.clone());
    let sample = Sample::from_read(
        collection.clone(),
        Read {
            complete: true,
            cutoff: now - WINDOW,
            records: [closed.clone(), settled.clone()]
                .into_iter()
                .map(|item| Record {
                    facts: WorkflowFacts {
                        native_state: Some(NativeStateProvenance::Tracker),
                        ..Default::default()
                    },
                    item,
                })
                .collect(),
            truncated: false,
        },
        now,
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(&directory.path().join("private/history.sqlite")).unwrap();
    store.replace_history(&sample, &[], now).unwrap();
    let mut snapshot = snapshot(vec![open.clone()]);
    snapshot.history = Arc::new(vec![Status {
        collection,
        failure: None,
        origin: Some(Origin::Cached),
        sample: store.history().unwrap().pop(),
        supported: true,
    }]);
    let prepare = |snapshot, previous| {
        board_rows::prepare(
            vec![board_rows::Input {
                account: None,
                live: Default::default(),
                snapshot,
                worker: "history-worker".into(),
            }],
            Mappings::default(),
            board_rows::Filter {
                backends: Default::default(),
                projects: Default::default(),
                query: String::new(),
                repos: None,
            },
            1,
            None,
            previous,
            board_rows::LaneOrder::default(),
        )
        .unwrap()
    };
    let ready = Arc::new(prepare(snapshot.clone(), None));
    let cards = |board: &crate::orchestrator::view::kanban::Prepared| {
        board
            .pages
            .as_ref()
            .unwrap()
            .iter()
            .flat_map(|page| &page.cards)
            .map(|card| (card.key.clone(), card.column))
            .collect::<Vec<_>>()
    };
    let expected = vec![
        (open.key.canonical(), wire::BoardColumn::Open),
        (settled.key.canonical(), wire::BoardColumn::Done),
    ];
    assert_eq!(cards(&ready), expected);
    Arc::make_mut(&mut snapshot.history)[0].failure = Some(Arc::new(
        sheprd_orchestrator::Error::Kanban(sheprd_orchestrator::kanban::Error::HistoryIncomplete),
    ));
    let failed = prepare(snapshot, Some(ready));
    assert_eq!(cards(&failed), expected);
    let history = &failed.pages.as_ref().unwrap()[0].lanes[0].history[0];
    assert!(history.failed);
    assert_eq!(history.last_success, Some(now.to_rfc3339()));
}
