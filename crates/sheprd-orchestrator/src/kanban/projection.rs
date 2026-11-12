//! Prepare from immutable worker snapshots, never from render or provider I/O.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::{
    BoardColumn, Result,
    config::{LabelMap, Mappings},
    facts::{Placement, WorkflowFacts, place},
    identity::{CardKey, LaneKey, Route},
};
use crate::{Item, Provider, SourceKey};

pub const MAX_LANES: usize = 128;
pub const MAX_CARDS_PER_CELL: usize = 100;
pub const MAX_CARDS: usize = 2_000;

/// The existing roster supplies empty and failed repositories too. An account
/// supplies each item's repository, never the account collection source key.
pub struct LaneInput<'a, R> {
    pub route: &'a Route<R>,
    pub source: &'a SourceKey,
}

pub struct CardInput<'a, R> {
    pub facts: &'a WorkflowFacts,
    pub item: &'a Item,
    pub route: &'a Route<R>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card<R> {
    pub facts: WorkflowFacts,
    pub item: Item,
    pub key: CardKey,
    pub mapping: LabelMap,
    pub placement: Placement,
    pub route: Route<R>,
}

/// Counts refer to unique supported records in the supplied snapshots, not to
/// tracker-wide totals. Filtering does not change `loaded`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    pub displayed: usize,
    pub loaded: usize,
    pub matching: usize,
    pub tracker_total: Option<usize>,
}

impl Coverage {
    pub fn truncated(&self) -> bool {
        self.displayed < self.matching
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell<R> {
    pub cards: Vec<Card<R>>,
    pub column: BoardColumn,
    pub coverage: Coverage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lane<R> {
    pub cells: Vec<Cell<R>>,
    pub key: LaneKey,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Board<R> {
    pub cards: Coverage,
    pub lanes: Vec<Lane<R>>,
    pub roster: Coverage,
}

fn supported(source: &SourceKey) -> bool {
    matches!(source.provider, Provider::Github | Provider::Forgejo)
}

/// Stable canonical order. Filtering runs after checkout-preferred deduplication
/// but before every display cap. `matches` also receives empty lanes (`None`),
/// so a search can exclude them before they consume the lane limit.
/// No input is trimmed or mutated, including shared records of hidden providers.
pub fn project<'a, R: Clone + 'a>(
    roster: impl IntoIterator<Item = LaneInput<'a, R>>,
    cards: impl IntoIterator<Item = CardInput<'a, R>>,
    mappings: &Mappings,
    matches: impl Fn(&LaneKey, Option<&Item>) -> bool,
) -> Result<Board<R>> {
    project_ordered(roster, cards, mappings, &[], matches)
}

/// Apply local lane order before lane selection and the shared card budget.
/// Unlisted keys retain canonical order after the requested keys.
pub fn project_ordered<'a, R: Clone + 'a>(
    roster: impl IntoIterator<Item = LaneInput<'a, R>>,
    cards: impl IntoIterator<Item = CardInput<'a, R>>,
    mappings: &Mappings,
    order: &[LaneKey],
    matches: impl Fn(&LaneKey, Option<&Item>) -> bool,
) -> Result<Board<R>> {
    let mut lanes: BTreeMap<String, LaneKey> = BTreeMap::new();
    let mut checkouts = HashSet::new();
    for input in roster {
        if !supported(input.source) {
            continue;
        }
        input.route.validate(input.source)?;
        let lane = LaneKey::try_from(input.source)?;
        if input.route.is_checkout() {
            checkouts.insert(lane.clone());
        }
        lanes.insert(lane.canonical(), lane);
    }

    // Deduplication uses normalized lane identity; the selected ItemKey and
    // concrete route retain their original spelling and generation.
    let mut unique: BTreeMap<(String, String), CardInput<'a, R>> = BTreeMap::new();
    for input in cards {
        let source = &input.item.key.source;
        if !supported(source) {
            continue;
        }
        input.route.validate(source)?;
        let lane = LaneKey::try_from(source)?;
        if input.route.is_checkout() {
            checkouts.insert(lane.clone());
        }
        let canonical = lane.canonical();
        lanes.insert(canonical.clone(), lane);
        let key = (canonical, input.item.key.native_id.clone());
        match unique.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(input);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let held = entry.get();
                if (!held.route.is_checkout() && input.route.is_checkout())
                    || (held.route.is_checkout() == input.route.is_checkout()
                        && super::facts::prefer(input.item, input.facts, held.item, held.facts))
                {
                    entry.insert(input);
                }
            }
        }
    }
    unique.retain(|(lane, _), input| {
        input.route.is_checkout() || !lanes.get(lane).is_some_and(|key| checkouts.contains(key))
    });

    let mut board = Board {
        cards: Coverage {
            loaded: unique.len(),
            ..Coverage::default()
        },
        lanes: Vec::new(),
        roster: Coverage {
            loaded: lanes.len(),
            ..Coverage::default()
        },
    };
    let mut grouped: HashMap<String, Vec<CardInput<'a, R>>> = HashMap::new();
    for ((lane, _), input) in unique {
        grouped.entry(lane).or_default().push(input);
    }
    let ranks: HashMap<_, _> = order
        .iter()
        .enumerate()
        .map(|(at, key)| (key, at))
        .collect();
    let mut lanes: Vec<_> = lanes.into_iter().collect();
    lanes.sort_by_key(|(_, key)| ranks.get(key).copied().unwrap_or(usize::MAX));
    for (canonical, key) in lanes {
        let inputs = grouped.remove(&canonical).unwrap_or_default();
        let mut cells: Vec<Cell<R>> = BoardColumn::ALL
            .iter()
            .map(|column| Cell {
                cards: Vec::new(),
                column: *column,
                coverage: Coverage::default(),
            })
            .collect();
        let mut matching = inputs.is_empty() && matches(&key, None);
        for input in inputs {
            let mapping = mappings.for_source(&input.item.key.source)?;
            let placement = place(input.item, input.facts, mapping);
            let Some(cell) = cells
                .iter_mut()
                .find(|cell| cell.column == placement.column)
            else {
                continue;
            };
            cell.coverage.loaded += 1;
            if !matches(&key, Some(input.item)) {
                continue;
            }
            matching = true;
            board.cards.matching += 1;
            cell.coverage.matching += 1;
            if board.lanes.len() >= MAX_LANES
                || board.cards.displayed >= MAX_CARDS
                || cell.cards.len() >= MAX_CARDS_PER_CELL
            {
                continue;
            }
            cell.cards.push(Card {
                facts: input.facts.clone(),
                item: input.item.clone(),
                key: CardKey {
                    item: input.item.key.clone(),
                    lane: key.clone(),
                },
                mapping: mapping.clone(),
                placement,
                route: input.route.clone(),
            });
            cell.coverage.displayed += 1;
            board.cards.displayed += 1;
        }
        if matching {
            board.roster.matching += 1;
            if board.lanes.len() < MAX_LANES {
                board.lanes.push(Lane { cells, key });
                board.roster.displayed += 1;
            }
        }
    }
    Ok(board)
}
