use super::*;
use crate::{
    forgejo::{Page, Reply, SearchNode},
    kanban::{
        BoardColumn,
        config::Mappings,
        history::{Collection, MAX_ACCOUNT_RECORDS, MAX_RECORDS, WINDOW},
        identity::{LaneKey, Origin, Route},
        projection::{self, CardInput, LaneInput},
    },
};
use chrono::{DateTime, Utc};

fn closed(repository: &str, number: u64, now: DateTime<Utc>) -> SearchNode {
    serde_json::from_value(serde_json::json!({
        "number": number, "title": "Settled observation", "state": "closed",
        "html_url": format!("https://{HOST}/{repository}/issues/{number}"),
        "created_at": now - WINDOW, "updated_at": now,
        "closed_at": now, "labels": [],
        "repository": { "full_name": repository }
    }))
    .unwrap()
}

fn read(worker: &mut AccountWorker, failed: bool, now: DateTime<Utc>) {
    let collection = Collection::ForgejoAccount { host: HOST.into() };
    worker.history.refresh();
    worker.history.read(
        collection,
        super::super::super::kanban::ReadContext {
            active: &worker.snapshot.items,
            blocked_until: None,
            now: Instant::now(),
            observed_at: now,
            store: worker.store.as_mut(),
            visible: true,
        },
        |_| {
            forgejo::history::list_account(HOST, now - WINDOW, &mut |page| {
                if failed && page > 1 {
                    return Err(forgejo::Error::Status(503).into());
                }
                // Smaller configured pages, issue numbers shared by distinct repositories.
                let size = 30;
                let start = (page as usize - 1) * size;
                Ok(Reply::Page(Page {
                    nodes: (start..start + size)
                        .map(|i| {
                            closed(
                                if i % 2 == 0 { "team/app" } else { "team/web" },
                                (i / 2) as u64 + 1,
                                now,
                            )
                        })
                        .collect(),
                    total: Some((MAX_ACCOUNT_RECORDS * 2) as u64),
                }))
            })
            .map(Some)
        },
    );
    worker.history.publish(&mut worker.snapshot, now);
}

#[test]
fn capped_or_failed_account_history_preserves_open_items_and_repository_lanes() {
    let dir = tempfile::tempdir().unwrap();
    let now = Utc::now();
    let mut first = worker(dir.path());
    let open = vec![issue("team/app", 1), issue("team/web", 700)];
    first.keep(listing(open.clone(), true)).unwrap();
    read(&mut first, false, now);
    let status = &first.snapshot.history[0];
    let sample = status.sample.as_ref().unwrap();
    assert!(sample.coverage().truncated);
    assert!(sample.records().len() <= MAX_ACCOUNT_RECORDS);
    for repository in ["team/app", "team/web"] {
        assert!(
            sample
                .records()
                .iter()
                .filter(|record| record.item.key.source.repository == repository)
                .count()
                <= MAX_RECORDS
        );
    }
    let prior = sample.clone();
    read(&mut first, true, now + chrono::TimeDelta::seconds(1));
    assert!(first.snapshot.history[0].failure.is_some());
    assert_eq!(first.snapshot.history[0].sample.as_ref(), Some(&prior));
    assert_eq!(&*first.snapshot.items, &open);

    let restarted = worker(dir.path());
    assert_eq!(keys(&restarted.snapshot.items), keys(&open));
    let mut snapshot = restarted.snapshot.clone();
    let mut history = restarted.history;
    history.publish(&mut snapshot, now);
    let records = snapshot.board_records(now);
    let route = Route {
        origin: Origin::ForgejoAccount { host: HOST.into() },
        worker: (),
    };
    let sources = ["team/app", "team/web"].map(|repository| issue(repository, 1).key.source);
    let board = projection::project(
        sources.iter().map(|source| LaneInput {
            source,
            route: &route,
        }),
        records.iter().map(|record| CardInput {
            item: &record.item,
            facts: &record.facts,
            route: &route,
        }),
        &Mappings::default(),
        |_, _| true,
    )
    .unwrap();
    for lane in &board.lanes {
        for card in lane.cells.iter().flat_map(|cell| &cell.cards) {
            assert_eq!(lane.key, LaneKey::try_from(&card.item.key.source).unwrap());
            if open.iter().any(|item| item.key == card.item.key) {
                assert_eq!(card.placement.column, BoardColumn::Open);
            }
        }
    }
    for item in open {
        assert!(
            board
                .lanes
                .iter()
                .flat_map(|lane| &lane.cells)
                .flat_map(|cell| &cell.cards)
                .any(|card| card.item.key == item.key)
        );
    }
}
