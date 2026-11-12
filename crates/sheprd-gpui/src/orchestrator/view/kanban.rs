//! A separate presentation and selection over the existing repository workers.
pub(super) mod drag;
pub(crate) mod error;
mod history;
pub(crate) mod order;
mod popup;
mod render;
pub(super) mod rows;

use super::{Detail, DetailTab, OrchestratorView};
use crate::i18n::fl;
use gpui::{prelude::*, *};
pub(crate) use rows::Prepared;
use sheprd_orchestrator::kanban::{config::Mappings, identity::CardKey};
use std::{collections::HashSet, sync::Arc};

#[derive(Default)]
pub(super) struct State {
    pub(super) account_ready: bool,
    applied_order: Vec<sheprd_orchestrator::kanban::identity::LaneKey>,
    authoritative: bool,
    collapsed: HashSet<String>,
    pub(super) drag: drag::State,
    error: Option<error::Error>,
    focused_lane: Option<sheprd_orchestrator::kanban::identity::LaneKey>,
    horizontal: ScrollHandle,
    lines: Vec<rows::Line>,
    locale: Option<sheprd_i18n::Locale>,
    mapping: Mappings,
    pub(super) order: Vec<sheprd_orchestrator::kanban::identity::LaneKey>,
    order_chosen: bool,
    order_dirty: bool,
    popup: popup::Popup,
    positions: Vec<Option<(usize, usize)>>,
    prepared: Option<u64>,
    preparing: Option<u64>,
    ready: Option<Arc<Prepared>>,
    removed: bool,
    removed_task: Option<Task<()>>,
    requested: bool,
    pub(super) revision: u64,
    save_failed: bool,
    scroll: UniformListScrollHandle,
    selected: Option<CardKey>,
    pub shown: bool,
    visible: Vec<usize>,
}

impl State {
    pub(super) fn invalidate(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        // Keep the displayed rows and their scroll handles mounted during preparation.
        self.error = None;
    }

    fn rebuild_lines(&mut self) {
        self.lines.clear();
        self.visible.clear();
        self.positions.clear();
        let Some(ready) = &self.ready else {
            return;
        };
        self.positions.resize(ready.cards.len(), None);
        for (index, lane) in ready.lanes.iter().enumerate() {
            self.lines.push(rows::Line::Header(index));
            if !self.collapsed.contains(&lane.key.canonical()) {
                let count = lane
                    .cells
                    .iter()
                    .map(Vec::len)
                    .max()
                    .unwrap_or_default()
                    .max(5);
                for row in 0..count {
                    let line = self.lines.len();
                    self.lines.push(rows::Line::Cards { lane: index, row });
                    for cell in &lane.cells {
                        if let Some(card) = cell.get(row) {
                            self.positions[*card] = Some((line, self.visible.len()));
                            self.visible.push(*card);
                        }
                    }
                }
            }
        }
    }

    fn reveal_lane(&self) {
        if let Some(key) = &self.focused_lane
            && let Some(ready) = &self.ready
            && let Some(line) = self.lines.iter().position(
                |line| matches!(line, rows::Line::Header(index) if &ready.lanes[*index].key == key),
            )
        {
            self.scroll.scroll_to_item(line, ScrollStrategy::Nearest);
        }
    }

    pub(super) fn keys(&self) -> Vec<String> {
        self.ready
            .as_ref()
            .map(|ready| {
                self.visible
                    .iter()
                    .map(|index| ready.cards[*index].summary.key.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn card(&self, key: &str) -> Option<&rows::Card> {
        let ready = self.ready.as_ref()?;
        ready.cards.get(*ready.by_key.get(key)?)
    }

    pub(super) fn visible_position(&self, key: &str) -> Option<(usize, usize)> {
        let ready = self.ready.as_ref()?;
        self.positions
            .get(*ready.by_key.get(key)?)?
            .as_ref()
            .copied()
    }

    pub(super) fn detail_position(&self, key: &str) -> (Option<usize>, usize) {
        (
            self.visible_position(key).map(|(_, position)| position),
            self.visible.len(),
        )
    }

    fn reveal(&self, key: &str) {
        let Some((line, _)) = self.visible_position(key) else {
            return;
        };
        let Some(card) = self.card(key) else {
            return;
        };
        self.scroll.scroll_to_item(line, ScrollStrategy::Nearest);
        let left = px(render::REPOSITORY_WIDTH + render::COLUMN_WIDTH * card.column as f32);
        let width = self.horizontal.bounds().size.width;
        let offset = self.horizontal.offset();
        let start = -offset.x;
        let end = left + px(render::COLUMN_WIDTH);
        let next = if left < start || width < px(render::COLUMN_WIDTH) {
            left
        } else if end > start + width {
            end - width
        } else {
            start
        };
        self.horizontal
            .set_offset(point(-next.max(px(0.)), offset.y));
    }

    fn directional_key(&self, direction: &str) -> Option<String> {
        let ready = self.ready.as_ref()?;
        let current = self
            .selected
            .as_ref()
            .and_then(|key| self.card(&key.item.canonical()))
            .and_then(|card| {
                self.visible_position(&card.summary.key)
                    .map(|(line, _)| (card, line))
            });
        let Some((current, line)) = current else {
            return self
                .visible
                .first()
                .map(|index| ready.cards[*index].summary.key.clone());
        };
        self.visible
            .iter()
            .filter_map(|index| {
                let card = &ready.cards[*index];
                let (other_line, _) = self.positions[*index]?;
                let rank = match direction {
                    "up" | "k" if card.column == current.column && other_line < line => {
                        (line - other_line, 0)
                    }
                    "down" | "j" if card.column == current.column && other_line > line => {
                        (other_line - line, 0)
                    }
                    "left" if card.lane == current.lane && card.column < current.column => {
                        (current.column - card.column, card.row.abs_diff(current.row))
                    }
                    "right" if card.lane == current.lane && card.column > current.column => {
                        (card.column - current.column, card.row.abs_diff(current.row))
                    }
                    _ => return None,
                };
                Some((rank, card))
            })
            .min_by_key(|(rank, _)| *rank)
            .map(|(_, card)| card.summary.key.clone())
    }
}

impl OrchestratorView {
    pub(crate) fn kanban_shown(&self) -> bool {
        self.kanban.shown
    }

    pub(crate) fn show_kanban(&mut self, shown: bool, cx: &mut Context<Self>) {
        if self.kanban.shown != shown {
            self.kanban.shown = shown;
            self.detail = None;
            self.kanban.popup = popup::Popup::default();
            self.selected = None;
            self.merge_open = false;
            self.pr.track(None);
            if !shown {
                self.refresh_rows();
            }
        }
        if shown {
            self.kanban.requested = true;
            self.prepare_kanban(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_kanban_mapping(&mut self, mapping: &Mappings, cx: &mut Context<Self>) {
        if self.kanban.mapping != *mapping {
            self.kanban.mapping = mapping.clone();
            self.kanban.invalidate();
        }
        self.prepare_kanban(cx);
    }

    pub(crate) fn request_board(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<Option<Arc<Prepared>>, &error::Error> {
        self.kanban.requested = true;
        self.prepare_kanban(cx);
        if let Some(error) = &self.kanban.error {
            return Err(error);
        }
        Ok((self.kanban.prepared == Some(self.kanban.revision))
            .then(|| self.kanban.ready.clone())
            .flatten())
    }

    pub(super) fn prepare_kanban(&mut self, cx: &mut Context<Self>) {
        if !self.kanban.requested {
            return;
        }
        let locale = crate::i18n::shown_locale();
        if self.kanban.locale != locale {
            self.kanban.locale = locale;
            self.kanban.invalidate();
        }
        if self.kanban.preparing.is_some()
            || self.kanban.prepared == Some(self.kanban.revision)
            || self.kanban.error.is_some()
        {
            return;
        }
        let inputs = self
            .repos
            .iter()
            .map(|repo| rows::Input {
                worker: repo.key.clone(),
                snapshot: repo.snapshot.clone(),
                account: match &repo.remote {
                    Some(crate::orchestrator::RemoteScope::ForgejoAccount { host }) => {
                        Some(host.clone())
                    }
                    _ => None,
                },
            })
            .collect();
        let revision = self.kanban.revision;
        let order = rows::LaneOrder {
            authoritative: self.kanban.authoritative && self.kanban.account_ready,
            keys: self.kanban.order.clone(),
        };
        let mapping = self.kanban.mapping.clone();
        let filter = rows::Filter {
            query: self.query.clone(),
            projects: self.projects.clone(),
            backends: self.backends.clone(),
        };
        let selected = self.kanban.selected.clone();
        let previous = self.kanban.ready.clone();
        self.kanban.preparing = Some(revision);
        let task = cx.background_executor().spawn(async move {
            rows::prepare(inputs, mapping, filter, revision, selected, previous, order)
        });
        cx.spawn(async move |this, cx| {
            let prepared = task.await;
            let _ =
                this.update(cx, |this, cx| {
                    this.kanban.preparing = None;
                    if this.kanban.revision == revision {
                        match prepared {
                            Ok(ready) => {
                                if this.kanban.order != ready.order {
                                    this.kanban.order = ready.order.clone();
                                    this.kanban.order_dirty = true;
                                }
                                if this.kanban.focused_lane.as_ref().is_some_and(|key| {
                                    !ready.lanes.iter().any(|lane| &lane.key == key)
                                }) {
                                    this.kanban.focused_lane =
                                        ready.lanes.first().map(|lane| lane.key.clone());
                                }
                                this.kanban.prepared = Some(revision);
                                this.reconcile_kanban_detail(&ready, cx);
                                this.kanban.ready = Some(Arc::new(ready));
                                this.kanban.rebuild_lines();
                                this.kanban.drag.installed();
                            }
                            Err(error) => {
                                this.kanban.drag.failed();
                                this.kanban.error = Some(error);
                            }
                        }
                        cx.notify();
                    }
                    this.prepare_kanban(cx);
                });
        })
        .detach();
    }

    /// A stale route or cross-host key has no fallback to the list's first holder.
    pub(super) fn kanban_repo_of(&self, key: &str) -> Option<usize> {
        let ready = self.kanban.ready.as_ref()?;
        let card = self.kanban.card(key)?;
        // Projection validates the key and route. Only this immutable snapshot may use it.
        let index = card.domain.route.worker;
        let route = ready.routes.get(index)?;
        let repo = self.repos.get(index)?;
        (repo.key == route.worker
            && Arc::ptr_eq(&repo.snapshot.items, &route.items)
            && Arc::ptr_eq(&repo.snapshot.history, &route.history))
        .then_some(index)
    }

    fn reconcile_kanban_detail(&mut self, ready: &Prepared, cx: &mut Context<Self>) {
        let Some(tracked) = &ready.tracked else {
            return;
        };
        if self.kanban.selected.as_ref() != Some(&tracked.key) {
            return;
        }
        if let Some(item) = &tracked.item {
            if let Some(card) = &mut self.kanban.popup.card
                && card.domain.key == tracked.key
            {
                if let Some(index) = ready.by_key.get(&item.key.canonical()) {
                    *card = ready.cards[*index].clone();
                } else {
                    card.domain.item = item.clone();
                }
            }
        } else if tracked.complete {
            self.kanban.selected = None;
            if self.kanban.shown && self.detail.is_some() {
                self.detail = None;
                self.kanban.popup.close();
                self.kanban.removed = true;
                self.kanban.removed_task = Some(cx.spawn(async |this, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(5))
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        this.kanban.removed = false;
                        cx.notify();
                    });
                }));
            }
        }
    }

    pub(super) fn kanban_item(&self, key: &str) -> Option<&sheprd_orchestrator::Item> {
        self.kanban_repo_of(key)?;
        Some(&self.kanban.card(key)?.domain.item)
    }

    pub(super) fn step_kanban_detail(&mut self, by: isize, cx: &mut Context<Self>) {
        let Some((_, at)) = self
            .detail
            .as_ref()
            .and_then(|detail| self.kanban.visible_position(&detail.key))
        else {
            return;
        };
        let next = at
            .saturating_add_signed(by)
            .min(self.kanban.visible.len() - 1);
        let key = self
            .kanban
            .ready
            .as_ref()
            .map(|ready| ready.cards[self.kanban.visible[next]].summary.key.clone());
        if let Some(key) = key {
            self.open_kanban_key(&key, cx);
        }
    }

    pub(super) fn open_kanban_key(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.kanban.visible_position(key).is_none() {
            return;
        }
        let Some(repo) = self.kanban_repo_of(key) else {
            return;
        };
        let Some(card) = self.kanban.card(key).cloned() else {
            return;
        };
        self.kanban.selected = Some(card.domain.key.clone());
        self.kanban.popup.open(card, cx);
        self.kanban.removed = false;
        self.kanban.reveal(key);
        // A running projection must also check the newly selected identity.
        if self.kanban.preparing.is_some() {
            self.kanban.invalidate();
        }
        self.active = repo;
        self.detail = Some(Detail {
            key: key.into(),
            tab: DetailTab::Description,
        });
        self.merge_open = false;
        self.pr.track(None);
        cx.notify();
    }

    pub(super) fn kanban_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.kanban.popup.is_shown() {
            self.kanban_popup_key(event, window, cx);
            return true;
        }
        if self.search.read(cx).focus.is_focused(window) {
            if event.keystroke.key == "escape" {
                window.focus(&self.focus, cx);
                return true;
            }
            return false;
        }
        if event.keystroke.key == "escape" && self.kanban.drag.active.is_some() {
            self.kanban.drag.cancel();
            cx.stop_active_drag(window);
            window.focus(&self.focus, cx);
            cx.notify();
            return true;
        }
        if event.keystroke.modifiers.alt && matches!(event.keystroke.key.as_str(), "up" | "down") {
            self.step_lane(event.keystroke.key == "up", cx);
            return true;
        }
        if event.keystroke.key == "tab" {
            self.focus_lane(event.keystroke.modifiers.shift);
            cx.notify();
            return true;
        }
        if event.keystroke.modifiers.platform
            || event.keystroke.modifiers.control
            || event.keystroke.modifiers.alt
        {
            return false;
        }
        match event.keystroke.key.as_str() {
            "escape" => window.focus(&self.focus, cx),
            "/" => window.focus(&self.search.read(cx).focus.clone(), cx),
            "enter" => {
                if let Some(key) = self
                    .kanban
                    .selected
                    .as_ref()
                    .map(|key| key.item.canonical())
                {
                    self.open_kanban_key(&key, cx);
                    self.focus_kanban_popup(window, cx);
                }
            }
            "up" | "down" | "left" | "right" | "j" | "k" => {
                if let Some(key) = self.kanban.directional_key(&event.keystroke.key) {
                    self.kanban.selected =
                        self.kanban.card(&key).map(|card| card.domain.key.clone());
                    self.kanban.reveal(&key);
                    if self.kanban.preparing.is_some() {
                        self.kanban.invalidate();
                    }
                }
            }
            _ => return true,
        }
        cx.notify();
        true
    }
}
