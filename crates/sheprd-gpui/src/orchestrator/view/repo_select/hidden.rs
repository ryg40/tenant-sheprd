//! What the selection hides is no longer acted on. A change closes the open
//! page, dialog, confirmation, and board popup or menu of a hidden
//! repository, and each action checks its target again before it goes out.
//! A write already handed to a worker is left to finish: nothing here
//! cancels or replays one.

use super::{OrchestratorView, checkout_identity, identity};
use crate::orchestrator::view::dispatch::Confirm;
use gpui::{Context, FocusHandle, Window};
use sheprd_orchestrator::{ItemKey, kanban::identity::LaneKey};

impl OrchestratorView {
    /// Whether the item `key` belongs only to repositories the selection
    /// hides. An item no repository lists is judged by its own source.
    pub(in crate::orchestrator::view) fn item_hidden(&self, key: &str) -> bool {
        if self.repo_select.active().is_none() {
            return false;
        }
        let mut listed = false;
        for repo in &self.repos {
            if let Some(item) = repo.item(key) {
                if self.repo_shown(repo, item) {
                    return false;
                }
                listed = true;
            }
        }
        listed
            || key.parse::<ItemKey>().map_or(true, |key| {
                !self.repo_select.shows(Some(&identity(&key.source)))
            })
    }

    /// Whether the item `key` is hidden as the active repository lists it.
    /// Every action of a page, a dispatch, or a run goes to that
    /// repository's worker, so a copy another repository shows does not
    /// make it shown.
    pub(in crate::orchestrator::view) fn bound_hidden(&self, key: &str) -> bool {
        if self.repo_select.active().is_none() {
            return false;
        }
        let Some(repo) = self.repo() else {
            return true;
        };
        if let Some(item) = repo.item(key) {
            return !self.repo_shown(repo, item);
        }
        // An item its worker no longer lists: the checkout's identity, or
        // for an account the item's own source.
        let shown = if repo.remote.is_some() {
            key.parse::<ItemKey>().ok().map(|key| identity(&key.source))
        } else {
            checkout_identity(repo)
        };
        !self.repo_select.shows(shown.as_deref())
    }

    /// Whether the open page's item is hidden as the worker its actions go
    /// to lists it, so its comment, merge, and message go nowhere.
    pub(in crate::orchestrator::view) fn open_item_hidden(&self) -> bool {
        self.detail
            .as_ref()
            .is_some_and(|detail| self.bound_hidden(&detail.key))
    }

    /// Whether the selection shows the board lane `lane`.
    pub(in crate::orchestrator::view) fn lane_shown(&self, lane: &LaneKey) -> bool {
        self.repo_select.shows(Some(&lane.canonical()))
    }

    /// Whether `confirm` acts on a hidden repository's item. A merge acts
    /// on the open page's pull request; a stop or a removal on its run's
    /// item, through the worker shown when it was asked, so another
    /// worker shown since counts as hidden too, and so does a copy of the
    /// item only another worker shows.
    pub(in crate::orchestrator::view) fn confirm_hidden(&self, confirm: &Confirm) -> bool {
        match confirm {
            Confirm::DeleteBead { source, id } => self.item_hidden(
                &ItemKey {
                    source: source.clone(),
                    native_id: id.clone(),
                }
                .canonical(),
            ),
            Confirm::Merge { .. } => self.detail.is_none() || self.open_item_hidden(),
            Confirm::Stop { item, repo, .. } | Confirm::Remove { item, repo, .. } => {
                self.repo().is_none_or(|shown| shown.key != *repo) || self.bound_hidden(item)
            }
        }
    }

    /// Closes what acts on a repository the selection now hides.
    pub(super) fn retire_hidden(&mut self, cx: &mut Context<Self>) {
        if self.repo_select.active().is_none() {
            return;
        }
        if self
            .confirm
            .as_ref()
            .is_some_and(|confirm| self.confirm_hidden(confirm))
        {
            self.confirm = None;
        }
        if self
            .dialog
            .as_ref()
            .is_some_and(|dialog| self.bound_hidden(dialog.item()))
            && let Some(dialog) = self.dialog.take()
        {
            let fields = dialog.focus_handles(cx);
            self.retire_focus(fields);
        }
        if !self.kanban.shown && self.open_item_hidden() {
            self.detail = None;
            self.merge_open = false;
        }
        self.retire_hidden_cards();
        cx.notify();
    }

    /// Keeps the focus handles of a context hiding closed, so the next
    /// tick can tell whether the focus was in it.
    pub(in crate::orchestrator::view) fn retire_focus(
        &mut self,
        handles: impl IntoIterator<Item = FocusHandle>,
    ) {
        self.repo_select.retired_focus.extend(handles);
    }

    /// Called from the host's tick: when the focus was in what hiding
    /// closed, it goes back to the view, so keys reach it again. Focus
    /// anywhere else, the picker's included, stays where it is.
    pub(crate) fn restore_retired_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let retired = std::mem::take(&mut self.repo_select.retired_focus);
        if retired.iter().any(|handle| handle.is_focused(window)) {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }
}
