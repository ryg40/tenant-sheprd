//! Each lane's order of implementation: a local rank, like the lane order.
//! Neither the reducer nor its persistence path has tracker actions. No read
//! proves that an item left its tracker, so an item's rank leaves only with
//! its lane or when the user or an agent clears or replaces it.
use super::{OrchestratorView, error, order};
use gpui::Context;
use sheprd_orchestrator::{ItemKey, kanban::identity::LaneKey};
use std::collections::{HashMap, HashSet};

/// Items one lane may rank, as one `board rank` may name them.
pub(crate) const MAX_ITEMS: usize = sheprd_control_protocol::protocol::orchestrator::MAX_RANK_ITEMS;
/// Ranked items across every lane: the board shows at most 2,000 cards.
pub(crate) const MAX_TOTAL: usize = 2_000;

/// Each lane's ranked native item IDs, first to build first. Lanes and items
/// keep at most [`order::MAX_LANES`], [`MAX_ITEMS`], and [`MAX_TOTAL`]
/// entries, and an ID at most [`order::MAX_KEY_BYTES`] bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Ranks(Vec<(LaneKey, Vec<String>)>);

/// A native ID as it may be stored: any other is dropped, never cut.
fn storable(id: &str) -> bool {
    !id.is_empty() && id.len() <= order::MAX_KEY_BYTES && !id.chars().any(char::is_control)
}

impl Ranks {
    /// Drops what the bounds refuse: later duplicates, empty lists, and what
    /// exceeds a count, in stored order.
    pub(crate) fn bounded(entries: impl IntoIterator<Item = (LaneKey, Vec<String>)>) -> Self {
        let mut lanes: Vec<(LaneKey, Vec<String>)> = Vec::new();
        let mut total = 0;
        for (lane, ids) in entries {
            if lanes.len() == order::MAX_LANES || total == MAX_TOTAL {
                break;
            }
            if lane.canonical().len() > order::MAX_KEY_BYTES
                || lanes.iter().any(|(held, _)| *held == lane)
            {
                continue;
            }
            let mut kept: Vec<String> = Vec::new();
            for id in ids {
                if kept.len() == MAX_ITEMS || total + kept.len() == MAX_TOTAL {
                    break;
                }
                if storable(&id) && !kept.contains(&id) {
                    kept.push(id);
                }
            }
            if !kept.is_empty() {
                total += kept.len();
                lanes.push((lane, kept));
            }
        }
        Self(lanes)
    }

    pub(crate) fn lanes(&self) -> impl Iterator<Item = (&LaneKey, &[String])> {
        self.0.iter().map(|(lane, ids)| (lane, ids.as_slice()))
    }

    pub(crate) fn get(&self, lane: &LaneKey) -> &[String] {
        self.0
            .iter()
            .find(|(held, _)| held == lane)
            .map_or(&[], |(_, ids)| ids.as_slice())
    }

    /// Where each ranked item stands in its lane, for sorting cells.
    pub(crate) fn index(&self) -> HashMap<&LaneKey, HashMap<&str, usize>> {
        self.0
            .iter()
            .map(|(lane, ids)| {
                let at = ids
                    .iter()
                    .enumerate()
                    .map(|(at, id)| (id.as_str(), at))
                    .collect();
                (lane, at)
            })
            .collect()
    }

    /// A lane's ranks leave with the lane, on the evidence that prunes lanes
    /// (`authoritative` and its absence from `roster`). An item absent from
    /// a read keeps its rank: active and history reads happen at different
    /// times, so no absence proves that the item left its tracker.
    pub(crate) fn reconcile(&self, roster: &[LaneKey], authoritative: bool) -> Self {
        Self::bounded(
            self.0
                .iter()
                .filter(|(lane, _)| !authoritative || roster.contains(lane))
                .cloned(),
        )
    }

    /// Replaces one lane's whole order; an empty list clears it. A repeated
    /// or unstorable ID refuses with `OrderLimit`, a count over a bound with
    /// `RankLimit`.
    pub(crate) fn set(&self, lane: &LaneKey, ids: Vec<String>) -> Result<Self, error::Error> {
        let mut seen = HashSet::new();
        if ids
            .iter()
            .any(|id| !storable(id) || !seen.insert(id.as_str()))
        {
            return Err(error::Error::OrderLimit);
        }
        if ids.len() > MAX_ITEMS {
            return Err(error::Error::RankLimit);
        }
        let others: usize = self
            .0
            .iter()
            .filter(|(held, _)| held != lane)
            .map(|(_, ids)| ids.len())
            .sum();
        let lanes = self.0.len() + usize::from(self.get(lane).is_empty());
        if others + ids.len() > MAX_TOTAL || (!ids.is_empty() && lanes > order::MAX_LANES) {
            return Err(error::Error::RankLimit);
        }
        let mut next = self.0.clone();
        match next.iter_mut().find(|(held, _)| held == lane) {
            Some((_, held)) => *held = ids,
            None => next.push((lane.clone(), ids)),
        }
        next.retain(|(_, ids)| !ids.is_empty());
        Ok(Self(next))
    }

    /// Moves `item` to place `to` of a cell shown in `cell` order, ranked
    /// cards first. Every card the cell then shows before it, and every card
    /// already ranked, holds a rank, so the cell keeps the order the user
    /// dropped it in; cards after it stay unranked.
    pub(crate) fn arrange(
        &self,
        lane: &LaneKey,
        cell: &[String],
        item: &str,
        to: usize,
    ) -> Result<Self, error::Error> {
        let at = cell
            .iter()
            .position(|id| id == item)
            .ok_or(error::Error::UnknownCard)?;
        let list = self.get(lane);
        let ranked = cell.iter().filter(|id| list.contains(id)).count();
        let mut shown = cell.to_vec();
        let moved = shown.remove(at);
        let to = to.min(shown.len());
        if to == at && list.iter().any(|id| id == item) {
            return Ok(self.clone());
        }
        shown.insert(to, moved);
        let held = ranked + usize::from(!list.iter().any(|id| id == item));
        shown.truncate((to + 1).max(held).min(shown.len()));
        // The cell's ranked cards keep the lane positions they hold, so
        // ranks of the lane's other cells keep their places between them.
        let slots: Vec<_> = list
            .iter()
            .enumerate()
            .filter(|(_, id)| cell.contains(id))
            .map(|(at, _)| at)
            .collect();
        let mut next = list.to_vec();
        for (slot, id) in slots.iter().zip(&shown) {
            next[*slot] = id.clone();
        }
        let after = slots.last().map_or(next.len(), |slot| slot + 1);
        next.splice(after..after, shown.into_iter().skip(slots.len()));
        self.set(lane, next)
    }

    pub(crate) fn cleared(&self, lane: &LaneKey, item: &str) -> Result<Self, error::Error> {
        let next = self
            .get(lane)
            .iter()
            .filter(|id| *id != item)
            .cloned()
            .collect();
        self.set(lane, next)
    }
}

/// The number each ranked card shows: its place among the lane's ranked
/// cards that are loaded and not settled. `open` holds, for each lane, the
/// loaded native IDs that are not settled.
pub(crate) fn numbers(
    ranks: &Ranks,
    open: &HashMap<LaneKey, HashSet<String>>,
) -> HashMap<(LaneKey, String), usize> {
    let mut numbers = HashMap::new();
    for (lane, ids) in ranks.lanes() {
        let Some(open) = open.get(lane) else {
            continue;
        };
        for (at, id) in ids.iter().filter(|id| open.contains(*id)).enumerate() {
            numbers.insert((lane.clone(), id.clone()), at + 1);
        }
    }
    numbers
}

/// A command's ITEM: a canonical item key of `lane`, as board reads print it.
pub(crate) fn native_id(lane: &LaneKey, item: &str) -> Option<String> {
    let key: ItemKey = item.parse().ok()?;
    (key.canonical() == item && LaneKey::try_from(&key.source).ok().as_ref() == Some(lane))
        .then_some(key.native_id)
}

impl OrchestratorView {
    fn rank_ready(&self) -> Result<(), error::Error> {
        if self.dialog.is_some()
            || self.confirm.is_some()
            || self.merge_open
            || self.kanban.drag.active.is_some()
            || self.kanban.drag.active_card.is_some()
            || self.kanban.move_dialog.is_some()
            || self.kanban.move_menu_open()
        {
            return Err(error::Error::Busy);
        }
        if self.kanban.prepared != Some(self.kanban.revision) {
            return Err(error::Error::Pending);
        }
        Ok(())
    }

    fn apply_ranks(&mut self, next: Ranks, cx: &mut Context<Self>) {
        if next != self.kanban.ranks {
            self.kanban.ranks = next;
            self.kanban.order_dirty = true;
            self.kanban.order_chosen = true;
            self.kanban.invalidate();
            self.prepare_kanban(cx);
            cx.notify();
        }
    }

    /// Sets a lane's whole order from canonical item keys. Every key must
    /// name an item the lane loaded, or nothing changes.
    pub(crate) fn rank_lane(
        &mut self,
        lane: &LaneKey,
        items: &[String],
        cx: &mut Context<Self>,
    ) -> Result<(), error::Error> {
        self.rank_ready()?;
        let ready = self.kanban.ready.as_ref().ok_or(error::Error::Pending)?;
        // A rank is a lane's local preference, like its place: a hidden
        // lane keeps its saved ranks, but none of them is a target.
        if !ready.roster.contains(lane) || !self.lane_shown(lane) {
            return Err(error::Error::UnknownLane);
        }
        let loaded = ready.loaded_ids.get(lane);
        let ids = items
            .iter()
            .map(|item| {
                native_id(lane, item)
                    .filter(|id| loaded.is_some_and(|loaded| loaded.contains(id)))
                    .ok_or(error::Error::UnknownCard)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let next = self.kanban.ranks.set(lane, ids)?;
        self.apply_ranks(next, cx);
        Ok(())
    }

    /// Moves a shown card to place `to` of its own cell.
    pub(crate) fn rank_card(
        &mut self,
        key: &str,
        to: usize,
        cx: &mut Context<Self>,
    ) -> Result<(), error::Error> {
        self.rank_ready()?;
        let ready = self.kanban.ready.as_ref().ok_or(error::Error::Pending)?;
        let card = self.kanban.card(key).ok_or(error::Error::UnknownCard)?;
        let lane = &ready.lanes[card.lane];
        if !self.lane_shown(&lane.key) {
            return Err(error::Error::UnknownLane);
        }
        let cell: Vec<_> = lane.cells[card.column]
            .iter()
            .map(|index| ready.cards[*index].domain.item.key.native_id.clone())
            .collect();
        let next =
            self.kanban
                .ranks
                .arrange(&lane.key, &cell, &card.domain.item.key.native_id, to)?;
        self.kanban.selected = Some(card.domain.key.clone());
        self.apply_ranks(next, cx);
        Ok(())
    }

    /// Alt+Shift+Up or Down moves the selected card one place in its cell;
    /// Alt+Shift+Backspace removes its rank.
    pub(super) fn step_rank(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(selected) = self.kanban.selected.clone() else {
            return;
        };
        let item = selected.item.canonical();
        let Some(card) = self.kanban.card(&item) else {
            return;
        };
        let (row, native) = (card.row, card.domain.item.key.native_id.clone());
        let shown = self
            .kanban
            .ready
            .as_ref()
            .map_or(0, |ready| ready.lanes[card.lane].cells[card.column].len());
        let result = match key {
            "up" if row > 0 => self.rank_card(&item, row - 1, cx),
            "down" if row + 1 < shown => self.rank_card(&item, row + 1, cx),
            "backspace" => self.rank_ready().and_then(|()| {
                if !self.lane_shown(&selected.lane) {
                    return Err(error::Error::UnknownLane);
                }
                let next = self.kanban.ranks.cleared(&selected.lane, &native)?;
                self.apply_ranks(next, cx);
                Ok(())
            }),
            _ => Ok(()),
        };
        // A full order is the one refusal the user can act on here; a busy
        // or pending board refuses the key without a notice.
        if let Err(error @ error::Error::RankLimit) = result {
            self.kanban.error = Some(error);
            cx.notify();
        }
    }
}
