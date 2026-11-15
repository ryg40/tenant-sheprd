//! The repositories the orchestrator's sections show. The user picks the
//! active ones by the identity the board's lanes compare (host and
//! repository, without ASCII case on GitHub and Forgejo); no selection saved
//! means every repository is active. One predicate, [`State::shows`], decides
//! for the list rows, the board's lanes and cards, and the activity bar's
//! badges. It only narrows what is shown: workers keep syncing, since a
//! checkout's identity is known only from its worker's first read, and one
//! Forgejo account worker reads many repositories.

pub(crate) mod error;
mod hidden;
pub(super) mod picker;
mod roster;

use super::{OrchestratorView, repo::Repo};
use crate::orchestrator::{Item, SourceKey};
use error::Error;
use gpui::{App, Context, FocusHandle};
use sheprd_orchestrator::kanban::identity::LaneKey;
use std::{cell::Cell, collections::HashSet};

/// The most keys a saved selection keeps, active and hidden together.
pub(crate) const MAX_KEYS: usize = sheprd_control_protocol::protocol::orchestrator::MAX_REPO_KEYS;
/// The longest key, in UTF-8 bytes.
pub(crate) const MAX_KEY_BYTES: usize =
    sheprd_control_protocol::protocol::orchestrator::MAX_REPO_KEY_BYTES;

/// The identity a repository is selected by: a lane's canonical key on
/// GitHub and Forgejo, else the source's own canonical key.
pub(crate) fn identity(source: &SourceKey) -> String {
    LaneKey::try_from(source).map_or_else(|_| source.canonical(), |lane| lane.canonical())
}

/// `text` as a selection key, when it is one exactly as [`identity`] spells it.
pub(crate) fn parse(text: &str) -> Option<String> {
    if text.is_empty() || text.len() > MAX_KEY_BYTES {
        return None;
    }
    let source: SourceKey = text.parse().ok()?;
    (identity(&source) == text).then(|| text.to_owned())
}

/// A saved choice: what is active, and what was shown and left out. A key in
/// neither is a repository that appeared since, which the picker marks new.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub active: Vec<String>,
    pub hidden: Vec<String>,
}

impl Selection {
    /// The stored form, `{"active": [...], "hidden": [...]}`. Anything else,
    /// such as an object without both arrays, is no selection, so every
    /// repository shows rather than none.
    pub(crate) fn from_json(value: Option<&serde_json::Value>) -> Option<Self> {
        let object = value?.as_object()?;
        let keys = |name: &str| -> Option<Vec<String>> {
            let keys = object.get(name)?.as_array()?;
            Some(
                keys.iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .collect(),
            )
        };
        Some(Self::bounded(keys("active")?, keys("hidden")?))
    }

    pub(crate) fn to_json(&self) -> serde_json::Value {
        let bounded = Self::bounded(self.active.iter().cloned(), self.hidden.iter().cloned());
        serde_json::json!({ "active": bounded.active, "hidden": bounded.hidden })
    }

    /// Valid, distinct keys, active first, at most [`MAX_KEYS`] in all.
    pub(crate) fn bounded(
        active: impl IntoIterator<Item = String>,
        hidden: impl IntoIterator<Item = String>,
    ) -> Self {
        let mut selection = Self::valid(active, hidden);
        selection.active.truncate(MAX_KEYS);
        selection.hidden.truncate(MAX_KEYS - selection.active.len());
        selection
    }

    /// Valid, distinct keys, active first, with no bound.
    fn valid(
        active: impl IntoIterator<Item = String>,
        hidden: impl IntoIterator<Item = String>,
    ) -> Self {
        let mut seen = HashSet::new();
        let mut keep = |key: &String| parse(key).is_some() && seen.insert(key.clone());
        let active: Vec<_> = active.into_iter().filter(&mut keep).collect();
        let hidden: Vec<_> = hidden.into_iter().filter(&mut keep).collect();
        Self { active, hidden }
    }
}

/// The window's saved selection, which outlives its views and saves with
/// the rest of the chrome. Nothing prunes it: a host that is not connected,
/// or a listing that failed, proves no repository gone.
#[derive(Default)]
pub(crate) struct Stored {
    pub(crate) failed: Cell<bool>,
    /// A user choice outranks a load that arrives after it.
    pub(crate) modified: bool,
    pub(crate) pending: Cell<Option<u64>>,
    pub(crate) selection: Option<Selection>,
    pub(crate) unsaved: Cell<bool>,
}

impl Stored {
    pub(crate) fn restore(&mut self, selection: Option<Selection>) {
        if !self.modified {
            self.selection = selection;
        }
    }

    /// A save of revision `revision` finished; whether that settled this one.
    pub(crate) fn settle(&self, revision: u64, saved: bool) -> bool {
        if self.pending.get().is_none_or(|pending| revision < pending) {
            return false;
        }
        self.pending.set(None);
        self.failed.set(!saved);
        if saved {
            self.unsaved.set(false);
        }
        true
    }
}

/// One repository the picker and `orchestrator repos` list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) key: String,
    /// Its `owner/name`, or its folder.
    pub(crate) name: String,
    /// The forge's host, or the machine a Beads project is on.
    pub(crate) host: String,
    pub(crate) active: bool,
    /// Listed since the selection was saved, and not picked.
    pub(crate) new: bool,
}

/// What a selection change asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    All,
    Only(Vec<String>),
}

/// The view's copy of the selection, the roster, and the picker.
pub(crate) struct State {
    selection: Option<Selection>,
    active: Option<HashSet<String>>,
    /// Changed here, until the window collects it.
    dirty: bool,
    save_failed: bool,
    /// The last change from the picker was refused: it would pass
    /// [`MAX_KEYS`].
    full: bool,
    roster: roster::Roster,
    pub(super) picker: picker::Picker,
    /// The focus handles of what hiding closed, until the host's tick
    /// checks with the window whether one held the focus.
    retired_focus: Vec<FocusHandle>,
}

impl State {
    pub(crate) fn new(cx: &mut App) -> Self {
        Self {
            selection: None,
            active: None,
            dirty: false,
            save_failed: false,
            full: false,
            roster: roster::Roster::default(),
            picker: picker::Picker::new(cx),
            retired_focus: Vec::new(),
        }
    }

    /// Whether a repository of identity `key` shows; `None` is one whose
    /// identity is not known yet, which shows only without a selection.
    pub(crate) fn shows(&self, key: Option<&str>) -> bool {
        match &self.active {
            None => true,
            Some(active) => key.is_some_and(|key| active.contains(key)),
        }
    }

    /// The active keys, when a selection narrows anything.
    pub(super) fn active(&self) -> Option<&HashSet<String>> {
        self.active.as_ref()
    }

    fn set(&mut self, selection: Option<Selection>) {
        self.active = selection
            .as_ref()
            .map(|selection| selection.active.iter().cloned().collect());
        self.selection = selection;
        self.reflag();
    }
}

/// The identity of a checkout: its remote's, else its Beads project's.
/// `None` for a listing without a checkout, whose items each name theirs.
pub(super) fn checkout_identity(repo: &Repo) -> Option<String> {
    if repo.remote.is_some() {
        return None;
    }
    let info = repo.snapshot.repo.as_ref()?;
    match &info.remote {
        Some(remote) => Some(identity(&remote.source)),
        None => info.beads_source().map(|source| source.canonical()),
    }
}

/// The identity `item` is selected by, as `repo` lists it.
pub(super) fn item_identity(repo: &Repo, item: &Item) -> Option<String> {
    if repo.remote.is_some() {
        Some(identity(&item.key.source))
    } else {
        checkout_identity(repo)
    }
}

impl OrchestratorView {
    /// Takes the window's saved selection, unless one chosen here waits to
    /// be collected.
    pub(crate) fn set_repo_selection(
        &mut self,
        selection: Option<&Selection>,
        save_failed: bool,
        cx: &mut Context<Self>,
    ) {
        if self.repo_select.save_failed != save_failed {
            self.repo_select.save_failed = save_failed;
            cx.notify();
        }
        self.prepare_repo_roster(cx);
        if self.repo_select.dirty || self.repo_select.selection.as_ref() == selection {
            return;
        }
        self.repo_select.set(selection.cloned());
        self.selection_changed(cx);
    }

    #[cfg(test)]
    pub(in crate::orchestrator::view) fn repo_selection(&self) -> Option<&Selection> {
        self.repo_select.selection.as_ref()
    }

    /// The selection chosen here since the window last collected it.
    pub(crate) fn take_repo_selection(&mut self) -> Option<Option<Selection>> {
        std::mem::take(&mut self.repo_select.dirty).then(|| self.repo_select.selection.clone())
    }

    /// Whether `item`, as `repo` lists it, belongs to an active repository.
    pub(super) fn repo_shown(&self, repo: &Repo, item: &Item) -> bool {
        self.repo_select.active().is_none()
            || self.repo_select.shows(item_identity(repo, item).as_deref())
    }

    /// Every item of an active repository, for the activity bar's badges.
    pub(crate) fn shown_items(&self) -> impl Iterator<Item = &Item> {
        self.repos.iter().flat_map(move |repo| {
            repo.snapshot
                .items
                .iter()
                .filter(move |item| self.repo_shown(repo, item))
        })
    }

    /// Every repository listed, once, by name, as last prepared.
    pub(crate) fn repo_entries(&self) -> &[Entry] {
        self.repo_select.roster.entries()
    }

    /// Changes the selection, as the picker or `orchestrator repos set` asks.
    /// An unknown or oversized key, or a change that would keep more than
    /// [`MAX_KEYS`], refuses the whole change.
    pub(crate) fn choose_repos(
        &mut self,
        choice: Choice,
        cx: &mut Context<Self>,
    ) -> Result<(), Error> {
        let next = match choice {
            Choice::All => None,
            Choice::Only(keys) => {
                if keys.len() > MAX_KEYS {
                    return self.refused(Error::TooMany, cx);
                }
                let entries = self.repo_entries();
                if keys.iter().any(|key| {
                    parse(key).is_none() || !entries.iter().any(|entry| entry.key == *key)
                }) {
                    return Err(Error::Unknown);
                }
                match self.selection_of(entries, |key| keys.iter().any(|at| at.as_str() == key)) {
                    Ok(next) => Some(next),
                    Err(error) => return self.refused(error, cx),
                }
            }
        };
        self.apply_selection(next, cx);
        Ok(())
    }

    /// Flips repository `key` in the selection, as its checkbox does.
    pub(super) fn toggle_repo(&mut self, key: &str, cx: &mut Context<Self>) {
        let entries = self.repo_entries();
        let next = self.selection_of(entries, |at| {
            let on = entries.iter().any(|entry| entry.key == at && entry.active);
            if at == key { !on } else { on }
        });
        match next {
            Ok(next) => self.apply_selection(Some(next), cx),
            Err(error) => {
                let _ = self.refused(error, cx);
            }
        }
    }

    /// Shows that a change was refused; the selection stays as it was.
    fn refused(&mut self, error: Error, cx: &mut Context<Self>) -> Result<(), Error> {
        self.repo_select.full = true;
        cx.notify();
        Err(error)
    }

    /// The listed repositories with `active` deciding each, then every saved
    /// key not listed now, kept as it was. A selection that would hold more
    /// than [`MAX_KEYS`] is refused rather than cut, so no saved key is lost.
    fn selection_of(
        &self,
        entries: &[Entry],
        active: impl Fn(&str) -> bool,
    ) -> Result<Selection, Error> {
        let (on, off): (Vec<_>, Vec<_>) = entries.iter().partition(|entry| active(&entry.key));
        let listed = |key: &&String| !entries.iter().any(|entry| entry.key == **key);
        let saved = self.repo_select.selection.as_ref();
        let kept_on = saved
            .into_iter()
            .flat_map(|saved| saved.active.iter().filter(listed));
        let kept_off = saved
            .into_iter()
            .flat_map(|saved| saved.hidden.iter().filter(listed));
        let next = Selection::valid(
            on.into_iter()
                .map(|entry| entry.key.clone())
                .chain(kept_on.cloned()),
            off.into_iter()
                .map(|entry| entry.key.clone())
                .chain(kept_off.cloned()),
        );
        if next.active.len() + next.hidden.len() > MAX_KEYS {
            return Err(Error::TooMany);
        }
        Ok(next)
    }

    fn apply_selection(&mut self, next: Option<Selection>, cx: &mut Context<Self>) {
        self.repo_select.full = false;
        if self.repo_select.selection == next {
            cx.notify();
            return;
        }
        self.repo_select.set(next);
        self.repo_select.dirty = true;
        self.selection_changed(cx);
    }

    fn selection_changed(&mut self, cx: &mut Context<Self>) {
        self.retire_hidden(cx);
        self.refresh_rows();
        self.prepare_kanban(cx);
        cx.notify();
    }
}
