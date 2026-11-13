//! Local lane order. Neither the reducer nor its persistence path has tracker actions.
use super::{OrchestratorView, error};
use gpui::{Context, EntityId};
use sheprd_orchestrator::{SourceKey, kanban::identity::LaneKey};
use std::cell::Cell;

pub(crate) const MAX_LANES: usize = 512;
pub(crate) const MAX_KEY_BYTES: usize = 1024;

pub(crate) fn parse(text: &str) -> Option<LaneKey> {
    if text.len() > MAX_KEY_BYTES {
        return None;
    }
    let source: SourceKey = text.parse().ok()?;
    let key = LaneKey::try_from(&source).ok()?;
    (key.canonical() == text).then_some(key)
}

pub(crate) fn bounded(keys: impl IntoIterator<Item = LaneKey>) -> Vec<LaneKey> {
    let mut result = Vec::new();
    for key in keys {
        if key.canonical().len() <= MAX_KEY_BYTES && !result.contains(&key) {
            result.push(key);
            if result.len() == MAX_LANES {
                break;
            }
        }
    }
    result
}

/// Only a complete roster may prove removal. Filters and display caps never enter here.
pub(crate) fn reconcile(
    saved: &[LaneKey],
    roster: &[LaneKey],
    authoritative: bool,
) -> Vec<LaneKey> {
    let mut next: Vec<_> = saved
        .iter()
        .filter(|key| !authoritative || roster.contains(key))
        .cloned()
        .collect();
    let mut added: Vec<_> = roster
        .iter()
        .filter(|key| !next.contains(key))
        .cloned()
        .collect();
    added.sort_by_key(LaneKey::canonical);
    next.extend(added);
    bounded(next)
}

/// Validate both ends before changing anything, including when the source equals the target.
pub(crate) fn moved(
    saved: &[LaneKey],
    roster: &[LaneKey],
    lane: &LaneKey,
    before: Option<&LaneKey>,
) -> Result<Vec<LaneKey>, error::Error> {
    if !roster.contains(lane) || before.is_some_and(|key| !roster.contains(key)) {
        return Err(error::Error::UnknownLane);
    }
    let mut next = reconcile(saved, roster, false);
    if !next.contains(lane) || before.is_some_and(|key| !next.contains(key)) {
        return Err(error::Error::OrderLimit);
    }
    if before == Some(lane) {
        return Ok(next);
    }
    next.retain(|key| key != lane);
    let at = before
        .and_then(|key| next.iter().position(|entry| entry == key))
        .unwrap_or(next.len());
    next.insert(at, lane.clone());
    Ok(next)
}

/// Window-owned Chrome state, independent of any filtered board or pending drag.
#[derive(Default)]
pub(crate) struct Stored {
    pub(crate) failed: Cell<bool>,
    pub(crate) keys: Vec<LaneKey>,
    pub(crate) loaded: bool,
    /// User choice alone decides precedence over a late initial load.
    pub(crate) modified: bool,
    pub(crate) pending: Cell<Option<u64>>,
    /// Automatic append/prune and user moves both require save feedback.
    pub(crate) unsaved: Cell<bool>,
}

impl Stored {
    pub(crate) fn restore(&mut self, keys: Vec<LaneKey>) {
        if !self.modified {
            self.keys = keys;
        }
        self.loaded = true;
    }
}

impl OrchestratorView {
    pub(crate) fn set_lane_order(
        &mut self,
        keys: &[LaneKey],
        authoritative: bool,
        cx: &mut Context<Self>,
    ) {
        if self.kanban.order_dirty && self.kanban.order_chosen {
            return;
        }
        if self.kanban.applied_order != keys || self.kanban.authoritative != authoritative {
            self.kanban.order_dirty = false;
            self.kanban.applied_order = keys.to_vec();
            self.kanban.order = keys.to_vec();
            self.kanban.authoritative = authoritative;
            self.kanban.invalidate();
            self.prepare_kanban(cx);
        }
    }

    pub(crate) fn take_lane_order(&mut self) -> Option<(Vec<LaneKey>, bool)> {
        std::mem::take(&mut self.kanban.order_dirty).then(|| {
            self.kanban.applied_order = self.kanban.order.clone();
            (
                self.kanban.order.clone(),
                std::mem::take(&mut self.kanban.order_chosen),
            )
        })
    }

    pub(crate) fn lane_order_failed(&mut self, failed: bool, cx: &mut Context<Self>) {
        if self.kanban.save_failed != failed {
            self.kanban.save_failed = failed;
            cx.notify();
        }
    }

    pub(crate) fn order_lane(
        &mut self,
        lane: &LaneKey,
        before: Option<&LaneKey>,
        cx: &mut Context<Self>,
    ) -> Result<(), error::Error> {
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
        let ready = self.kanban.ready.as_ref().ok_or(error::Error::Pending)?;
        let next = moved(&self.kanban.order, &ready.roster, lane, before)?;
        if next != self.kanban.order {
            self.kanban.order = next;
            self.kanban.order_dirty = true;
            self.kanban.order_chosen = true;
            self.kanban.focused_lane = Some(lane.clone());
            self.kanban.drag.placed(lane.clone());
            self.kanban.invalidate();
            self.prepare_kanban(cx);
            cx.notify();
        }
        Ok(())
    }

    pub(in crate::orchestrator::view) fn drop_lane(
        &mut self,
        drag: &super::drag::LaneDrag,
        before: Option<&LaneKey>,
        cx: &mut Context<Self>,
    ) -> Result<(), error::Error> {
        self.validate_lane_drag(drag, cx.entity_id())?;
        self.kanban.drag.active = None;
        self.order_lane(&drag.lane, before, cx)
    }

    pub(super) fn validate_lane_drag(
        &self,
        drag: &super::drag::LaneDrag,
        board: EntityId,
    ) -> Result<(), error::Error> {
        if drag.board != board
            || self.kanban.prepared != Some(self.kanban.revision)
            || drag.revision != self.kanban.revision
            || self.kanban.drag.active.as_ref() != Some(drag)
        {
            return Err(error::Error::UnknownLane);
        }
        if !self
            .kanban
            .ready
            .as_ref()
            .is_some_and(|ready| ready.roster.contains(&drag.lane))
        {
            return Err(error::Error::UnknownLane);
        }
        Ok(())
    }

    pub(super) fn step_lane(&mut self, up: bool, cx: &mut Context<Self>) {
        let Some(lane) = self
            .kanban
            .focused_lane
            .clone()
            .or_else(|| self.kanban.selected.as_ref().map(|key| key.lane.clone()))
        else {
            return;
        };
        let Some(ready) = &self.kanban.ready else {
            return;
        };
        let ordered = reconcile(&self.kanban.order, &ready.roster, false);
        let known: Vec<_> = ordered
            .into_iter()
            .filter(|key| ready.roster.contains(key))
            .collect();
        let Some(at) = known.iter().position(|key| key == &lane) else {
            return;
        };
        let before = if up {
            if at == 0 {
                return;
            }
            known.get(at - 1)
        } else {
            if at + 1 == known.len() {
                return;
            }
            known.get(at + 2)
        }
        .cloned();
        let _ = self.order_lane(&lane, before.as_ref(), cx);
    }

    pub(super) fn focus_lane(&mut self, previous: bool) {
        let Some(ready) = &self.kanban.ready else {
            return;
        };
        if ready.lanes.is_empty() {
            return;
        }
        let at = self
            .kanban
            .focused_lane
            .as_ref()
            .and_then(|key| ready.lanes.iter().position(|lane| &lane.key == key));
        let next = at.map_or(0, |at| {
            if previous {
                at.saturating_sub(1)
            } else {
                (at + 1).min(ready.lanes.len() - 1)
            }
        });
        self.kanban.focused_lane = Some(ready.lanes[next].key.clone());
        self.kanban.reveal_lane();
    }
}
