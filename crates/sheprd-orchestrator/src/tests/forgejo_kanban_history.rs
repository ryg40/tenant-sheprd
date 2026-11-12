use super::*;
use crate::{
    forgejo::{IssueNode, Page, PullRequestNode, Reply, history},
    kanban::history::{Collection, MAX_RECORDS, Sample, WINDOW},
};
use serde_json::json;

fn issue(number: u64, now: DateTime<Utc>) -> IssueNode {
    serde_json::from_value(json!({
        "number": number, "title": "Closed issue", "state": "closed",
        "html_url": format!("https://forgejo.example.test/team/app/issues/{number}"),
        "created_at": now - WINDOW, "updated_at": now, "closed_at": now,
        "labels": []
    }))
    .unwrap()
}

fn empty_pulls(_: u32) -> Result<Reply<PullRequestNode>> {
    Ok(Reply::Page(Page {
        nodes: Vec::new(),
        total: Some(0),
    }))
}

#[test]
fn small_or_repeated_pages_never_replace_a_good_sample_with_incomplete_history() {
    let now = Utc::now();
    let cutoff = now - WINDOW;
    let source = SourceKey {
        provider: Provider::Forgejo,
        host: "forgejo.example.test".into(),
        repository: "team/app".into(),
    };
    let collection = Collection::Repository(source.clone());
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("database/state.sqlite3")).unwrap();
    let baseline = history::list_repository(
        &source,
        cutoff,
        &mut |_| {
            Ok(Reply::Page(Page {
                nodes: vec![issue(900, now)],
                total: Some(1),
            }))
        },
        &mut empty_pulls,
    )
    .unwrap();
    let baseline = Sample::from_read(collection.clone(), baseline, now).unwrap();
    store.replace_history(&baseline, &[], now).unwrap();

    // Small server pages produce a bounded sample, without admitting the tail.
    let size = 30;
    let read = history::list_repository(
        &source,
        cutoff,
        &mut |page| {
            let start = (page as usize - 1) * size;
            Ok(Reply::Page(Page {
                nodes: (start..start + size)
                    .map(|id| issue(id as u64, now))
                    .collect(),
                total: Some((MAX_RECORDS * 3) as u64),
            }))
        },
        &mut empty_pulls,
    )
    .unwrap();
    let sample = Sample::from_read(collection.clone(), read, now).unwrap();
    assert!(sample.coverage().truncated);
    assert!(sample.records().len() <= MAX_RECORDS);
    assert!(
        !sample
            .records()
            .iter()
            .any(|record| record.item.key.native_id == MAX_RECORDS.to_string())
    );

    // None of these page sets proves a new sample. The stored baseline stays intact.
    for failure in [
        "repeat",
        "small-budget",
        "unreported-small-budget",
        "growing-page",
        "missing",
        "failed",
        "total",
        "order",
        "state",
    ] {
        let result = history::list_repository(
            &source,
            cutoff,
            &mut |page| {
                if page > 1 && failure == "missing" {
                    return Ok(Reply::Missing);
                }
                if page > 1 && failure == "failed" {
                    return Err(crate::forgejo::Error::Status(503).into());
                }
                let number = if failure == "repeat" {
                    1
                } else {
                    u64::from(page)
                };
                let updated = if failure == "order" {
                    now + chrono::TimeDelta::seconds(i64::from(page))
                } else {
                    now
                };
                let mut node = json!({
                    "number": number, "title": "Observation", "state": "closed",
                    "html_url": "https://forgejo.example.test/team/app/issues/1",
                    "created_at": cutoff, "updated_at": updated
                });
                if failure == "state" {
                    node["state"] = "open".into();
                }
                let mut nodes = vec![serde_json::from_value(node).unwrap()];
                if failure == "growing-page" && page > 1 {
                    nodes.push(issue(800, now));
                }
                Ok(Reply::Page(Page {
                    nodes,
                    total: (failure != "unreported-small-budget").then_some(
                        if failure == "total" && page > 1 {
                            3
                        } else {
                            500
                        },
                    ),
                }))
            },
            &mut empty_pulls,
        )
        .and_then(|read| {
            let sample = Sample::from_read(collection.clone(), read, now)?;
            store.replace_history(&sample, &[], now)
        });
        assert!(result.is_err(), "{failure}");
        let stored = store.history().unwrap();
        assert_eq!(stored[0].records()[0].item, baseline.records()[0].item);
        assert_eq!(stored[0].coverage(), baseline.coverage());
    }
}
