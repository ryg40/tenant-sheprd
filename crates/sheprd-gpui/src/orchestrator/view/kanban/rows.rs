//! Immutable board input and prepared virtual rows. Runs only on the background executor.
use super::*;
use sheprd_control_protocol::protocol::{Response, orchestrator as wire};
use sheprd_orchestrator::{
    Item, Provider, Snapshot, SourceKey, SyncState,
    kanban::{
        facts::{LabelCompleteness, MappingWarning, NativeStateProvenance},
        history::{Record, Status},
        identity::{LaneKey, Origin, Route},
        projection::{self, CardInput, LaneInput},
    },
};
use std::{cell::RefCell, collections::HashMap};

pub(in crate::orchestrator::view) struct Input {
    pub worker: String,
    pub snapshot: Snapshot,
    pub account: Option<String>,
}

pub(in crate::orchestrator::view) struct Filter {
    pub query: String,
    pub projects: HashSet<String>,
    pub backends: HashSet<Provider>,
}

#[derive(Default)]
pub(in crate::orchestrator::view) struct LaneOrder {
    pub authoritative: bool,
    pub keys: Vec<LaneKey>,
}

#[derive(Clone)]
pub(super) struct Card {
    pub active_backed: bool,
    pub domain: projection::Card<usize>,
    pub summary: wire::Card,
    pub name: SharedString,
    pub element_id: SharedString,
    pub suffix: SharedString,
    pub warnings: SharedString,
    pub mapping: SharedString,
    pub lane: usize,
    pub column: usize,
    pub row: usize,
}

pub(super) struct RouteSnapshot {
    pub history: Arc<Vec<Status>>,
    pub items: Arc<Vec<Item>>,
    pub worker: String,
}

pub(super) struct Lane {
    pub cells: Vec<Vec<usize>>,
    pub counts: Vec<SharedString>,
    pub element_id: SharedString,
    pub evidence: SharedString,
    pub hint: SharedString,
    pub history: SharedString,
    pub host: SharedString,
    pub key: LaneKey,
    pub mapping: SharedString,
    pub name: SharedString,
    pub status: SharedString,
    pub title: SharedString,
}

#[derive(Clone, Copy)]
pub(super) enum Line {
    Header(usize),
    Cards { lane: usize, row: usize },
}

pub(super) struct Tracked {
    pub key: CardKey,
    pub item: Option<Item>,
    pub complete: bool,
    workers: Vec<String>,
}

pub(crate) struct Prepared {
    pub(super) by_key: HashMap<String, usize>,
    pub(super) cards: Vec<Card>,
    pub(super) history_notice: Option<SharedString>,
    pub(super) lanes: Vec<Lane>,
    pub(super) loaded: SharedString,
    pub(super) order: Vec<LaneKey>,
    pub(crate) pages: Result<Vec<wire::BoardPage>, error::Error>,
    pub(super) partial: bool,
    pub(super) roster: Vec<LaneKey>,
    pub(super) routes: Vec<RouteSnapshot>,
    pub(super) selected_routes: HashMap<(LaneKey, String), Route<usize>>,
    pub(super) totals: Vec<SharedString>,
    pub(super) tracked: Option<Tracked>,
}

pub(super) fn coverage(value: projection::Coverage) -> wire::Coverage {
    wire::Coverage {
        displayed: value.displayed,
        loaded: value.loaded,
        matching: value.matching,
        tracker_total: value.tracker_total,
    }
}

pub(super) fn column(column: wire::BoardColumn) -> String {
    match column {
        wire::BoardColumn::Open => fl!("kanban-open"),
        wire::BoardColumn::InProgress => fl!("kanban-progress"),
        wire::BoardColumn::InReview => fl!("kanban-review"),
        wire::BoardColumn::Done => fl!("kanban-done"),
    }
}

fn warning(warning: MappingWarning) -> (&'static str, String) {
    match warning {
        MappingWarning::ConflictingLabels => ("conflicting_labels", fl!("kanban-conflict")),
        MappingWarning::IncompleteLabels => ("incomplete_labels", fl!("kanban-incomplete-labels")),
        MappingWarning::UnknownLabels => ("unknown_labels", fl!("kanban-unknown-labels")),
        MappingWarning::UnknownNativeState => ("unknown_native_state", fl!("kanban-unknown-state")),
        MappingWarning::UnknownProvenance => {
            ("unknown_provenance", fl!("kanban-unknown-provenance"))
        }
    }
}

fn freshness(source: &SourceKey, snapshot: &Snapshot, account: bool) -> (wire::Freshness, String) {
    let status = snapshot
        .sources
        .iter()
        .find(|status| status.key == *source)
        .or_else(|| account.then(|| snapshot.sources.first()).flatten());
    let (state, last_success, label) = if snapshot.error.is_some() {
        ("failed", None, fl!("kanban-source-failed"))
    } else {
        match status.map(|status| &status.state) {
            Some(SyncState::Synced { at, .. }) => (
                "synced",
                Some(at.to_rfc3339()),
                fl!("kanban-source-synced", at = at.to_rfc3339()),
            ),
            Some(SyncState::Syncing { .. }) => ("syncing", None, fl!("kanban-source-syncing")),
            Some(SyncState::Failed(_)) => ("failed", None, fl!("kanban-source-failed")),
            Some(SyncState::Capped { .. }) => ("capped", None, fl!("kanban-source-capped")),
            Some(SyncState::SignIn) => ("sign_in", None, fl!("kanban-source-sign-in")),
            Some(SyncState::Unsupported) => ("unsupported", None, fl!("kanban-source-unsupported")),
            None => ("unknown", None, fl!("kanban-source-unknown")),
        }
    };
    (
        wire::Freshness {
            source: source.canonical(),
            state: state.into(),
            last_success,
        },
        label,
    )
}

pub(in crate::orchestrator::view) fn prepare(
    inputs: Vec<Input>,
    mappings: Mappings,
    filter: Filter,
    revision: u64,
    selected: Option<CardKey>,
    previous: Option<Arc<Prepared>>,
    lane_order: LaneOrder,
) -> Result<Prepared, error::Error> {
    // Keep the original worker identity fence, separate from retained read-only rows.
    let routes: Vec<_> = inputs
        .iter()
        .map(|input| RouteSnapshot {
            history: input.snapshot.history.clone(),
            items: input.snapshot.items.clone(),
            worker: input.worker.clone(),
        })
        .collect();
    let now = chrono::Utc::now();
    let mut observations: Vec<_> = inputs
        .iter()
        .map(|input| input.snapshot.board_records(now))
        .collect();
    let mut active: Vec<HashSet<_>> = inputs
        .iter()
        .map(|input| {
            input
                .snapshot
                .items
                .iter()
                .filter_map(|item| {
                    LaneKey::try_from(&item.key.source)
                        .ok()
                        .map(|lane| (lane, item.key.native_id.clone()))
                })
                .collect()
        })
        .collect();
    for ((input, observations), active) in inputs.iter().zip(&mut observations).zip(&mut active) {
        let Some(prior_worker) = previous.as_ref().and_then(|board| {
            board
                .routes
                .iter()
                .position(|route| route.worker == input.worker)
        }) else {
            continue;
        };
        let current: HashSet<_> = observations
            .iter()
            .filter_map(|record| {
                LaneKey::try_from(&record.item.key.source)
                    .ok()
                    .map(|lane| (lane, record.item.key.native_id.clone()))
            })
            .collect();
        for card in previous
            .iter()
            .flat_map(|board| &board.cards)
            .filter(|card| card.domain.route.worker == prior_worker && card.active_backed)
        {
            let lane = &card.domain.key.lane;
            if input.snapshot.error.is_none()
                && input.snapshot.sources.iter().any(|status| {
                    (LaneKey::try_from(&status.key).ok().as_ref() == Some(lane)
                        || input.account.is_some())
                        && matches!(status.state, SyncState::Synced { .. })
                })
            {
                continue;
            }
            let key = (lane.clone(), card.domain.item.key.native_id.clone());
            // Carry provenance across preparations, not through the last raw listing.
            // History-only cards never acquire this marker or bypass expiry.
            active.insert(key.clone());
            if !current.contains(&key) {
                observations.push(Record {
                    facts: card.domain.facts.clone(),
                    item: card.domain.item.clone(),
                });
            }
        }
    }
    let mut roster = Vec::new();
    let mut records = Vec::new();
    let mut sources = Vec::new();
    for (worker, input) in inputs.iter().enumerate() {
        let mut known: Vec<SourceKey> = input
            .snapshot
            .sources
            .iter()
            .map(|status| status.key.clone())
            .collect();
        if let Some(remote) = input
            .snapshot
            .repo
            .as_ref()
            .and_then(|repo| repo.remote.as_ref())
        {
            known.push(remote.source.clone());
        }
        known.extend(input.snapshot.history.iter().filter_map(history::source));
        known.extend(
            observations[worker]
                .iter()
                .map(|record| record.item.key.source.clone()),
        );
        known.retain(|source| {
            matches!(source.provider, Provider::Github | Provider::Forgejo)
                && (input.account.is_none() || LaneKey::try_from(source).is_ok())
        });
        known.sort_by_key(SourceKey::canonical);
        known.dedup();
        for source in known {
            let route = Route {
                worker,
                origin: match &input.account {
                    Some(host) => Origin::ForgejoAccount { host: host.clone() },
                    None => Origin::Checkout(source.clone()),
                },
            };
            sources.push((source, route, input));
        }
    }
    for (source, route, _) in &sources {
        roster.push(LaneInput { source, route });
        for record in observations[route.worker]
            .iter()
            .filter(|record| record.item.key.source == *source)
        {
            records.push(CardInput {
                item: &record.item,
                route,
                facts: &record.facts,
            });
        }
    }
    // Keep the authoritative worker names even when a pending snapshot loses
    // its source metadata. Only leaving the current roster proves worker removal.
    let tracked = selected.map(|key| {
        let prior_workers = previous
            .as_ref()
            .map(|board| {
                if let Some(tracked) = &board.tracked
                    && tracked.key == key
                {
                    return tracked.workers.clone();
                }
                board
                    .by_key
                    .get(&key.item.canonical())
                    .and_then(|index| board.routes.get(board.cards[*index].domain.route.worker))
                    .map(|route| vec![route.worker.clone()])
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let mut matching: Vec<_> = inputs
            .iter()
            .filter(|input| {
                input.account.is_none()
                    && (prior_workers.contains(&input.worker)
                        || sources.iter().any(|(source, _, owner)| {
                            owner.worker == input.worker
                                && LaneKey::try_from(source).ok().as_ref() == Some(&key.lane)
                        }))
            })
            .collect();
        // Projection suppresses every account record in a checkout-owned lane.
        if matching.is_empty() {
            matching.extend(inputs.iter().filter(|input| {
                key.item.source.provider == Provider::Forgejo
                    && input
                        .account
                        .as_ref()
                        .is_some_and(|host| host.eq_ignore_ascii_case(&key.item.source.host))
            }));
        }
        let complete = if matching.is_empty() {
            !prior_workers.is_empty()
        } else {
            matching.iter().all(|input| {
                input.snapshot.error.is_none()
                    && history::complete(&key, &input.snapshot, input.account.is_some())
                    && input.snapshot.sources.iter().any(|status| {
                        (LaneKey::try_from(&status.key).ok().as_ref() == Some(&key.lane)
                            || input.account.is_some())
                            && matches!(status.state, SyncState::Synced { .. })
                    })
            })
        };
        let workers = if matching.is_empty() {
            prior_workers
        } else {
            matching.iter().map(|input| input.worker.clone()).collect()
        };
        Tracked {
            key,
            item: None,
            complete,
            workers,
        }
    });
    let tracked = RefCell::new(tracked);
    let words: Vec<_> = filter
        .query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();
    let mut full_roster: Vec<_> = sources
        .iter()
        .filter_map(|(source, _, _)| LaneKey::try_from(source).ok())
        .collect();
    full_roster.sort_by_key(LaneKey::canonical);
    full_roster.dedup();
    // Account issue searches cannot prove a repository disappeared. Failed, capped,
    // and loading sources likewise retain the saved keys for later recovery.
    let complete = lane_order.authoritative
        && inputs.iter().all(|input| {
            input.account.is_none()
                && input.snapshot.error.is_none()
                && input.snapshot.repo.is_some()
                && input
                    .snapshot
                    .sources
                    .iter()
                    .all(|status| matches!(status.state, SyncState::Synced { .. }))
        });
    let order = order::reconcile(&lane_order.keys, &full_roster, complete);
    let checkout_lanes: HashSet<_> = sources
        .iter()
        .filter(|(_, _, input)| input.account.is_none())
        .filter_map(|(source, _, _)| LaneKey::try_from(source).ok())
        .collect();
    let history_observations = history::Observations::new(
        inputs
            .iter()
            .map(|input| (input.account.is_none(), &input.snapshot)),
        &checkout_lanes,
        now,
    );
    let history_loaded = RefCell::new(HashMap::new());
    let board = projection::project_ordered(roster, records, &mappings, &order, |lane, item| {
        if let Some(item) = item {
            history_observations.count(lane, item, &mut history_loaded.borrow_mut());
        }
        // Projection has resolved identity and source precedence here, but has
        // not applied search or display caps. Suppressed copies cannot keep a popup.
        if let Some(tracked) = &mut *tracked.borrow_mut()
            && *lane == tracked.key.lane
            && let Some(item) = item
            && item.key.native_id == tracked.key.item.native_id
        {
            tracked.item = Some(item.clone());
        }
        let picked = sources.iter().any(|(source, _, input)| {
            let pick = if input.account.is_some() {
                super::super::projects::forgejo_repository(source)
            } else {
                input.worker.clone()
            };
            (filter.projects.is_empty() || filter.projects.contains(&pick))
                && (filter.backends.is_empty() || filter.backends.contains(&source.provider))
                && LaneKey::try_from(source).ok().as_ref() == Some(lane)
        });
        if !picked {
            return false;
        }
        let source = lane.source();
        let text = item
            .map(|item| searchable(item, &source))
            .unwrap_or_else(|| format!("{} {}", source.host, source.repository))
            .to_lowercase();
        words.iter().all(|word| text.contains(word))
    })?;
    let mut prepared = Prepared {
        by_key: HashMap::new(),
        cards: Vec::new(),
        history_notice: None,
        lanes: Vec::new(),
        loaded: fl!(
            "kanban-loaded",
            displayed = board.cards.displayed,
            loaded = board.cards.loaded
        )
        .into(),
        order,
        pages: Ok(Vec::new()),
        partial: board.cards.truncated() || board.roster.truncated(),
        roster: full_roster,
        routes,
        selected_routes: board.selected_routes,
        totals: Vec::new(),
        tracked: tracked.into_inner(),
    };
    if prepared.partial
        && let Some(tracked) = &mut prepared.tracked
    {
        tracked.complete = false;
    }
    let mut wire_lanes = Vec::new();
    let mut totals = vec![0; wire::BoardColumn::ALL.len()];
    for (position, lane) in board.lanes.iter().enumerate() {
        let source = lane.key.source();
        let canonical = lane.key.canonical();
        let mut lane_mappings = Vec::new();
        let mut active_sources = Vec::new();
        let mut statuses = Vec::new();
        let mut history_coverage = Vec::new();
        let checkout = checkout_lanes.contains(&lane.key);
        for (concrete, _, input) in &sources {
            if LaneKey::try_from(concrete).ok().as_ref() != Some(&lane.key) {
                continue;
            }
            if !checkout || input.account.is_none() {
                let coverage =
                    history::coverage(concrete, &input.snapshot, input.account.is_some());
                if !history_coverage.contains(&coverage) {
                    history_coverage.push(coverage);
                }
            }
            let mapping = mappings.for_source(concrete)?;
            let record = wire::Mapping {
                source: concrete.canonical(),
                in_progress_label: mapping.in_progress_label().into(),
                in_review_label: mapping.in_review_label().into(),
            };
            if !lane_mappings.contains(&record) {
                lane_mappings.push(record);
            }
            let (status, text) = freshness(concrete, &input.snapshot, input.account.is_some());
            if !active_sources.contains(&status) {
                active_sources.push(status);
            }
            if !statuses.contains(&text) {
                statuses.push(text);
            }
        }
        let mapping = lane_mappings
            .iter()
            .map(|mapping| {
                fl!(
                    "kanban-mapping",
                    progress = mapping.in_progress_label.clone(),
                    review = mapping.in_review_label.clone()
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        let evidence = [
            MappingWarning::UnknownProvenance,
            MappingWarning::UnknownLabels,
        ]
        .into_iter()
        .filter(|warning| {
            lane.cells
                .iter()
                .flat_map(|cell| &cell.cards)
                .any(|card| card.placement.warnings.contains(warning))
        })
        .map(|value| warning(value).1)
        .collect::<Vec<_>>()
        .join("; ");
        let mut cells = Vec::new();
        for (index, cell) in lane.cells.iter().enumerate() {
            totals[index] += cell.coverage.displayed;
            let mut cards = Vec::new();
            for card in &cell.cards {
                let mut warnings: Vec<_> = card
                    .placement
                    .warnings
                    .iter()
                    .copied()
                    .map(warning)
                    .collect();
                let (title, cut) = bounded_title(&card.item.title);
                if cut {
                    warnings.push(("title_truncated", fl!("kanban-title-cut")));
                }
                let warning_text = warnings
                    .iter()
                    .map(|(_, text)| text.as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                let suffix = match card.item.state.as_str() {
                    "closed" => fl!("kanban-closed"),
                    "merged" => fl!("kanban-merged"),
                    _ => warnings
                        .iter()
                        .find(|(code, _)| {
                            matches!(
                                *code,
                                "conflicting_labels" | "unknown_native_state" | "incomplete_labels"
                            )
                        })
                        .map(|(_, text)| text.clone())
                        .unwrap_or_default(),
                };
                let status = inputs[card.route.worker]
                    .snapshot
                    .moves
                    .iter()
                    .find(|status| status.card == card.key);
                let can_move = move_status::available(&card.item);
                let move_label = status.map(move_status::label).unwrap_or_else(|| {
                    if can_move {
                        fl!("kanban-move-proposal")
                    } else {
                        fl!("kanban-move-unavailable")
                    }
                });
                let suffix = status.map(move_status::label).unwrap_or(suffix);
                let summary = wire::Card {
                    key: card.key.item.canonical(),
                    lane: canonical.clone(),
                    identifier: card.item.identifier.clone(),
                    title,
                    column: cell.column,
                    native_state: card.item.state.clone(),
                    native_state_provenance: card.facts.native_state.map(|value| match value {
                        NativeStateProvenance::CachedTracker => "cached_tracker".into(),
                        NativeStateProvenance::Tracker => "tracker".into(),
                    }),
                    label_completeness: card.facts.labels.map(|value| match value {
                        LabelCompleteness::Complete => "complete".into(),
                        LabelCompleteness::Incomplete => "incomplete".into(),
                    }),
                    warnings: warnings.into_iter().map(|(code, _)| code.into()).collect(),
                    can_move,
                    read_only_reason: if can_move {
                        String::new()
                    } else {
                        "unsupported_move".into()
                    },
                };
                let name = fl!(
                    "kanban-card-name",
                    title = card.item.title.clone(),
                    source = canonical.clone(),
                    column = column(cell.column),
                    status = move_label
                )
                .into();
                prepared
                    .by_key
                    .insert(summary.key.clone(), prepared.cards.len());
                let row = cards.len();
                cards.push(prepared.cards.len());
                prepared.cards.push(Card {
                    active_backed: active[card.route.worker]
                        .contains(&(card.key.lane.clone(), card.item.key.native_id.clone())),
                    domain: card.clone(),
                    element_id: format!("kanban-card-{}", summary.key).into(),
                    summary,
                    name,
                    suffix: suffix.into(),
                    warnings: warning_text.into(),
                    mapping: fl!(
                        "kanban-mapping",
                        progress = card.mapping.in_progress_label(),
                        review = card.mapping.in_review_label()
                    )
                    .into(),
                    lane: position,
                    column: index,
                    row,
                });
            }
            cells.push(cards);
        }
        if history_coverage.iter().any(|coverage| !coverage.supported)
            && source.provider == Provider::Github
        {
            prepared.history_notice = Some(fl!("kanban-history-github").into());
        }
        let history_text = history::text(
            &history_coverage,
            history_loaded
                .borrow()
                .get(&lane.key)
                .copied()
                .unwrap_or_default(),
        );
        let history_hint = history::hint(&history_coverage);
        wire_lanes.push(wire::Lane {
            key: canonical.clone(),
            position,
            cells: lane
                .cells
                .iter()
                .map(|cell| coverage(cell.coverage))
                .collect(),
            mappings: lane_mappings,
            active_sources,
            history: history_coverage,
        });
        prepared.lanes.push(Lane {
            cells,
            counts: lane
                .cells
                .iter()
                .map(|cell| {
                    fl!(
                        "kanban-loaded",
                        displayed = cell.coverage.displayed,
                        loaded = cell.coverage.loaded
                    )
                    .into()
                })
                .collect(),
            element_id: format!("kanban-lane-{canonical}").into(),
            evidence: if evidence.is_empty() {
                String::new()
            } else {
                fl!("kanban-evidence-unknown")
            }
            .into(),
            hint: format!(
                "{canonical}\n{}\n{mapping}\n{evidence}\n{history_text}\n{history_hint}",
                statuses.join("; ")
            )
            .into(),
            history: history_text.into(),
            host: source.host.into(),
            key: lane.key.clone(),
            mapping: mapping.into(),
            name: fl!("kanban-lane-name", source = canonical.clone()).into(),
            status: statuses.join("; ").into(),
            title: source.repository.into(),
        });
    }
    prepared.totals = totals
        .into_iter()
        .map(|total| total.to_string().into())
        .collect();
    let page = wire::BoardPage {
        revision,
        columns: wire::BoardColumn::ALL.to_vec(),
        roster: coverage(board.roster),
        coverage: coverage(board.cards),
        lanes: Vec::new(),
        cards: Vec::new(),
        next_cursor: None,
        expanded_done_history: wire_lanes
            .iter()
            .any(|lane| lane.history.iter().any(|history| history.supported)),
    };
    prepared.pages = pages(page, wire_lanes, &prepared.cards);
    Ok(prepared)
}

/// Reserve most of each page for keys and metadata. Count JSON escapes, not UTF-8 alone.
fn bounded_title(title: &str) -> (String, bool) {
    let budget = wire::MAX_PAGE_BYTES / 4;
    let mut encoded = 0;
    for (at, ch) in title.char_indices() {
        encoded += match ch {
            '"' | '\\' | '\n' | '\r' | '\t' | '\u{0008}' | '\u{000c}' => 2,
            '\u{0000}'..='\u{001f}' => 6,
            _ => ch.len_utf8(),
        };
        // Include the quotes and the visible cut mark in the bound.
        if encoded + 5 > budget {
            return (format!("{}...", &title[..at]), true);
        }
    }
    (title.to_owned(), false)
}

fn searchable(item: &Item, source: &SourceKey) -> String {
    format!(
        "{} {} {} {} {} {}",
        source.host,
        source.repository,
        item.identifier,
        item.title,
        item.labels.join(" "),
        item.assignee.as_deref().unwrap_or_default()
    )
}

/// Page metadata too. Measure the actual response envelope, reserving the longest cursor.
fn pages(
    base: wire::BoardPage,
    lanes: Vec<wire::Lane>,
    cards: &[Card],
) -> Result<Vec<wire::BoardPage>, error::Error> {
    let mut result = Vec::new();
    let mut page = base.clone();
    for lane in lanes {
        page.lanes.push(lane);
        if page_size(&page)? > wire::MAX_PAGE_BYTES {
            let lane = page.lanes.pop().ok_or(error::Error::TooLarge)?;
            if page.lanes.is_empty() && page.cards.is_empty() {
                return Err(error::Error::TooLarge);
            }
            result.push(page);
            page = base.clone();
            page.lanes.push(lane);
            if page_size(&page)? > wire::MAX_PAGE_BYTES {
                return Err(error::Error::TooLarge);
            }
        }
    }
    for card in cards {
        page.cards.push(card.summary.clone());
        if page.cards.len() > wire::MAX_PAGE_CARDS || page_size(&page)? > wire::MAX_PAGE_BYTES {
            let card = page.cards.pop().ok_or(error::Error::TooLarge)?;
            if page.lanes.is_empty() && page.cards.is_empty() {
                return Err(error::Error::TooLarge);
            }
            result.push(page);
            page = base.clone();
            page.cards.push(card);
            if page_size(&page)? > wire::MAX_PAGE_BYTES {
                return Err(error::Error::TooLarge);
            }
        }
    }
    result.push(page);
    Ok(result)
}

fn page_size(page: &wire::BoardPage) -> Result<usize, error::Error> {
    let mut page = page.clone();
    page.next_cursor = Some("0".repeat(wire::MAX_CURSOR_BYTES));
    Ok(serde_json::to_vec(&Response::Orchestrator {
        reply: wire::BoardReply::Page(Box::new(page)),
    })?
    .len())
}
