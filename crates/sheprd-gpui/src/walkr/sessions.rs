//! What the Walkr section keeps of the saved sessions: the list, the pick,
//! and the detail of a picked row. The reads run on [`worker`]; this state
//! only sends requests, takes answers, and drops a late one.
mod worker;

#[cfg(test)]
#[path = "tests/history_deadline.rs"]
mod tests;

pub(crate) use worker::{DETAIL_LIMIT, LIST_LIMIT};

use super::{Error, history};
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};
use worker::{Answer, Worker};

/// How often a shown section lists again.
pub(crate) const REFRESH: Duration = Duration::from_secs(60);
/// How long past its time limit the window waits for the worker's own
/// answer before it drops the read.
const GRACE: Duration = Duration::from_secs(1);

/// The content for a picked row that is not live.
#[derive(Debug)]
pub(crate) enum Shown {
    Loading,
    /// Shared, so a page of it clones the handle, not the record.
    Detail(std::sync::Arc<history::Detail>),
    Failed(history::Error),
}

#[derive(Debug, Default)]
pub(crate) struct Sessions {
    /// A status call wants a list until then.
    demand: Option<Instant>,
    /// The session the detail is for, and what it shows.
    detail: Option<(String, Shown)>,
    detail_generation: u64,
    detail_reload: bool,
    /// The requested read deadline; GRACE covers delivery only.
    detail_waiting: Option<Instant>,
    list_generation: u64,
    list_attempted_at: Option<Instant>,
    list_failed_at: Option<Instant>,
    /// The requested read deadline; GRACE covers delivery only.
    list_waiting: Option<Instant>,
    list_wanted: bool,
    /// When the last successful list answered.
    listed_at: Option<Instant>,
    /// The last list was not read in time.
    list_timed_out: bool,
    pick: Option<String>,
    ratings: usize,
    retiring: Option<Worker>,
    rows: Vec<history::Row>,
    pub(crate) scroll: gpui::ScrollHandle,
    start_error: Option<Error>,
    /// The section's width at its last layout, for the narrow layout.
    pub(crate) width: Rc<Cell<f32>>,
    worker: Option<Worker>,
}

impl Sessions {
    /// A list is due: the section showed, or a run connected or ended.
    pub(crate) fn want_list(&mut self) {
        self.list_wanted = true;
    }

    /// Sends what is due and takes the answers. `live` is the session that
    /// a live run shows, whose row shows the page instead of a detail.
    /// Whether anything shown changed.
    pub(crate) fn poll(&mut self, shown: bool, live: Option<&str>, now: Instant) -> bool {
        self.reap();
        if self.demand.is_some_and(|until| now >= until) {
            self.demand = None;
        }
        if !shown && self.demand.is_none() {
            self.stop();
            return false;
        }
        let mut changed = false;
        if shown
            && self.list_waiting.is_none()
            && self
                .list_attempted_at
                .is_some_and(|at| now.saturating_duration_since(at) >= REFRESH)
        {
            self.list_wanted = true;
        }
        if self.worker.is_none() && self.start_error.is_none() {
            match worker::start() {
                Ok(worker) => self.worker = Some(worker),
                Err(error) => {
                    self.start_error = Some(error);
                    self.list_wanted = false;
                    self.list_failed_at = Some(now);
                    return true;
                }
            }
        }
        let Some(worker) = &self.worker else {
            return changed;
        };
        if worker.cancelled() {
            // Both slots are occupied by cancelled reads. Keep requests
            // pending until a handle finishes; never start a third thread.
            let changed = !self.list_timed_out;
            self.list_timed_out = true;
            self.list_failed_at = Some(now);
            return changed;
        }
        if self.list_wanted && self.list_waiting.is_none() {
            let generation = self.list_generation.wrapping_add(1);
            if worker.list(generation, now + LIST_LIMIT) {
                self.list_generation = generation;
                self.list_wanted = false;
                self.list_attempted_at = Some(now);
                self.list_waiting = Some(now + LIST_LIMIT);
            }
        }
        let wanted = self.pick.as_deref().filter(|pick| Some(*pick) != live);
        if let Some(id) = wanted
            && (self.detail_reload || self.detail.as_ref().is_none_or(|(known, _)| known != id))
        {
            let generation = self.detail_generation.wrapping_add(1);
            if worker.detail(
                generation,
                id.to_owned(),
                self.ratings.max(history::RATINGS_PAGE),
                now + DETAIL_LIMIT,
            ) {
                self.detail_generation = generation;
                if !self.detail_reload {
                    self.detail = Some((id.to_owned(), Shown::Loading));
                }
                self.detail_reload = false;
                self.detail_waiting = Some(now + DETAIL_LIMIT);
                changed = true;
            }
        }
        let mut answers = Vec::new();
        while let Ok(answer) = worker.answers.try_recv() {
            answers.push(answer);
        }
        for answer in answers {
            changed |= self.apply(answer, now);
        }
        changed | self.give_up(now)
    }

    fn apply(&mut self, answer: Answer, now: Instant) -> bool {
        match answer {
            Answer::List {
                generation,
                finished,
                rows,
            } => {
                let Some(deadline) = self.list_waiting else {
                    return false;
                };
                if generation != self.list_generation {
                    return false;
                }
                self.list_waiting = None;
                if finished > deadline
                    || rows
                        .iter()
                        .any(|row| row.state == history::RowState::TimedOut)
                {
                    self.list_timed_out = true;
                    self.list_failed_at = Some(now);
                } else {
                    self.listed_at = Some(now);
                    self.list_timed_out = false;
                    self.rows = rows;
                }
            }
            Answer::Detail {
                generation,
                finished,
                result,
            } => {
                let Some(deadline) = self.detail_waiting else {
                    return false;
                };
                if generation != self.detail_generation {
                    return false;
                }
                self.detail_waiting = None;
                let result = if finished > deadline {
                    Err(history::Error::TimedOut)
                } else {
                    result
                };
                if let Some((_, shown)) = &mut self.detail {
                    *shown = match result {
                        Ok(detail) => Shown::Detail(detail),
                        Err(error) => Shown::Failed(error),
                    };
                }
            }
        }
        true
    }

    /// Cancels an overdue worker without joining it. Two retained handles
    /// bound blocked reads even across timeouts and hide/show cycles.
    fn give_up(&mut self, now: Instant) -> bool {
        let list = self.list_waiting.is_some_and(|until| now >= until + GRACE);
        let detail = self
            .detail_waiting
            .is_some_and(|until| now >= until + GRACE);
        if !list && !detail {
            return false;
        }
        if list {
            self.list_waiting = None;
            self.list_generation = self.list_generation.wrapping_add(1);
            self.list_timed_out = true;
            self.list_failed_at = Some(now);
        }
        if detail {
            self.detail_waiting = None;
            self.detail_generation = self.detail_generation.wrapping_add(1);
            if let Some((_, shown)) = &mut self.detail {
                *shown = Shown::Failed(history::Error::TimedOut);
            }
        }
        self.stop();
        true
    }

    /// The section hides and no status call asks: the worker ends, and what
    /// it was asked is asked again the next time.
    fn stop(&mut self) {
        if let Some(worker) = &self.worker {
            worker.cancel();
        }
        self.reap();
        self.start_error = None;
        if self.list_waiting.take().is_some() {
            self.list_generation = self.list_generation.wrapping_add(1);
            self.list_wanted = true;
        }
        if self.detail_waiting.take().is_some() {
            self.detail_generation = self.detail_generation.wrapping_add(1);
            // The detail is asked again for the pick.
            self.detail = None;
        }
    }

    /// A cancelled worker stays owned until it ends. A second cancellation
    /// fills the active slot and prevents a third thread from starting.
    fn reap(&mut self) {
        if self.retiring.as_ref().is_some_and(Worker::is_finished) {
            self.retiring = None;
        }
        if self.worker.as_ref().is_some_and(Worker::is_finished) {
            self.worker = None;
        }
        if self.retiring.is_none() && self.worker.as_ref().is_some_and(Worker::cancelled) {
            self.retiring = self.worker.take();
        }
    }

    /// A status call wants the list until `until`.
    pub(crate) fn demand(&mut self, until: Instant) {
        self.demand = Some(self.demand.map_or(until, |known| known.max(until)));
    }

    /// A list answered now with `rows`, as the worker's answer would.
    #[cfg(test)]
    pub(crate) fn test_listed(&mut self, rows: Vec<history::Row>) {
        self.rows = rows;
        self.listed_at = Some(Instant::now());
        self.list_wanted = false;
    }

    /// Row `detail.id` is picked and read, as the worker's answer would.
    #[cfg(test)]
    pub(crate) fn test_detail(&mut self, detail: history::Detail) {
        self.pick = Some(detail.id.clone());
        self.detail = Some((
            detail.id.clone(),
            Shown::Detail(std::sync::Arc::new(detail)),
        ));
    }

    /// The listed rows, newest first.
    pub(crate) fn rows(&self) -> &[history::Row] {
        &self.rows
    }

    /// Whether the last list was dropped at its time limit.
    pub(crate) fn list_timed_out(&self) -> bool {
        self.list_timed_out
    }

    /// Whether a list succeeded since the window opened.
    pub(crate) fn listed(&self) -> bool {
        self.listed_at.is_some()
    }

    pub(crate) fn listed_at(&self) -> Option<Instant> {
        self.listed_at
    }

    /// Whether a list answered since `since`.
    pub(crate) fn listed_since(&self, since: Instant) -> bool {
        !self.list_timed_out
            && self.start_error.is_none()
            && self.listed_at.is_some_and(|at| {
                at >= since && self.list_failed_at.is_none_or(|failed| at > failed)
            })
    }

    /// Whether the last list is newer than [`REFRESH`] and none is due.
    pub(crate) fn fresh(&self, now: Instant) -> bool {
        !self.list_wanted
            && !self.list_timed_out
            && self.start_error.is_none()
            && self.list_waiting.is_none()
            && self.listed_at.is_some_and(|at| {
                now.saturating_duration_since(at) < REFRESH
                    && self.list_failed_at.is_none_or(|failed| at > failed)
            })
    }

    pub(crate) fn failed_since(&self, since: Instant) -> bool {
        self.list_failed_at.is_some_and(|at| at >= since)
    }

    pub(crate) fn error(&self) -> Option<&Error> {
        self.start_error.as_ref()
    }

    /// The picked row.
    pub(crate) fn pick(&self) -> Option<&str> {
        self.pick.as_deref()
    }

    /// Picks a row. A pick reads its session again, unless that read runs.
    pub(crate) fn choose(&mut self, id: String) {
        let loading = self
            .detail
            .as_ref()
            .is_some_and(|(known, shown)| *known == id && matches!(shown, Shown::Loading));
        if !loading {
            self.detail = None;
            self.detail_waiting = None;
            self.detail_reload = false;
            self.ratings = history::RATINGS_PAGE;
            self.scroll
                .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
        }
        self.pick = Some(id);
    }

    pub(crate) fn loading_detail(&self) -> bool {
        self.detail_reload || self.detail_waiting.is_some()
    }

    /// Keep the current detail and scroll offset while the larger page reads.
    pub(crate) fn earlier_ratings(&mut self) {
        if self.loading_detail() {
            return;
        }
        if let Some(Shown::Detail(detail)) = self.detail()
            && detail.ratings.len() < detail.ratings_total
        {
            self.ratings = detail.ratings.len().saturating_add(history::RATINGS_PAGE);
            self.detail_reload = true;
        }
    }

    /// Back to the page: no row is picked.
    pub(crate) fn back(&mut self) {
        self.pick = None;
    }

    /// The detail of the pick, while it is the pick's.
    pub(crate) fn detail(&self) -> Option<&Shown> {
        let pick = self.pick.as_deref()?;
        self.detail
            .as_ref()
            .filter(|(id, _)| id == pick)
            .map(|(_, shown)| shown)
    }
}

/// The session whose row is live: the run is live (a pane holds it), its
/// last probe answered, and its address names that session. Nothing else
/// makes a row live.
pub(crate) fn live_session(
    run: Option<&super::run::Run>,
    state: super::Connectivity,
) -> Option<&str> {
    let run = run.filter(|run| run.live())?;
    let answered = matches!(
        state,
        super::Connectivity::Reachable | super::Connectivity::Ready
    );
    answered.then(|| run.session())?
}
