//! The repositories the picker, `orchestrator repos`, and a change's checks
//! read. Their keys and names are prepared on the background executor from
//! what the workers read, and a build for an older revision is dropped. A
//! selection change only sets the `active` and `new` flags of the prepared
//! entries; it scans no item or history. Drawing and commands read the
//! prepared copy. A Forgejo account's repositories come from its listed
//! items and from the settled records of its board history, which the
//! workers already hold; nothing reads a provider for them.

use super::{Entry, OrchestratorView, Selection, State, checkout_identity, identity};
use crate::orchestrator::{Item, SourceKey, repo::RepoInfo};
use gpui::Context;
use sheprd_orchestrator::kanban::history::Status;
use std::{collections::HashSet, sync::Arc};

/// What one repository's part of the roster was built from. The shared
/// lists are held, not their addresses, so a freed list's address reused by
/// a new one cannot pass for it.
struct Mark {
    key: String,
    listing: bool,
    named: bool,
    checkout: Option<String>,
    items: Arc<Vec<Item>>,
    history: Arc<Vec<Status>>,
}

impl PartialEq for Mark {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
            && self.listing == other.listing
            && self.named == other.named
            && self.checkout == other.checkout
            && Arc::ptr_eq(&self.items, &other.items)
            && Arc::ptr_eq(&self.history, &other.history)
    }
}

/// One repository's part of a build, taken from its snapshot.
enum Input {
    Checkout {
        info: Box<RepoInfo>,
        name: String,
        host: String,
    },
    Listing {
        items: Arc<Vec<Item>>,
        history: Arc<Vec<Status>>,
    },
}

#[derive(Default)]
pub(super) struct Roster {
    entries: Vec<Entry>,
    /// How many entries are active, and how many are new.
    shown: usize,
    new: usize,
    /// What the newest revision is built from; `None` before the first.
    marks: Option<Vec<Mark>>,
    /// Bumped when the marks change.
    revision: u64,
    /// The revision `entries` holds, and the one being built.
    built: u64,
    preparing: Option<u64>,
}

impl Roster {
    pub(super) fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub(super) fn shown(&self) -> usize {
        self.shown
    }

    pub(super) fn new_count(&self) -> usize {
        self.new
    }

    /// Sets each entry's flags from the selection: one lookup per entry.
    fn reflag(&mut self, active: Option<&HashSet<String>>, saved: Option<&Selection>) {
        let saved: Option<HashSet<&str>> = saved.map(|saved| {
            saved
                .active
                .iter()
                .chain(&saved.hidden)
                .map(String::as_str)
                .collect()
        });
        for entry in &mut self.entries {
            entry.active = active.is_none_or(|active| active.contains(&entry.key));
            entry.new = saved
                .as_ref()
                .is_some_and(|saved| !saved.contains(entry.key.as_str()));
        }
        self.shown = self.entries.iter().filter(|entry| entry.active).count();
        self.new = self.entries.iter().filter(|entry| entry.new).count();
    }
}

impl State {
    /// The selection changed: the prepared entries take its flags.
    pub(super) fn reflag(&mut self) {
        self.roster
            .reflag(self.active.as_ref(), self.selection.as_ref());
    }
}

impl OrchestratorView {
    /// Starts a build when what the roster is built from changed since the
    /// last one. The check is one step per repository, not per item.
    pub(in crate::orchestrator::view) fn prepare_repo_roster(&mut self, cx: &mut Context<Self>) {
        let marks: Vec<Mark> = self
            .repos
            .iter()
            .map(|repo| Mark {
                key: repo.key.clone(),
                listing: repo.remote.is_some(),
                named: !repo.outside_git(),
                checkout: checkout_identity(repo),
                items: repo.snapshot.items.clone(),
                history: repo.snapshot.history.clone(),
            })
            .collect();
        let roster = &mut self.repo_select.roster;
        if roster.marks.as_ref() != Some(&marks) {
            roster.marks = Some(marks);
            roster.revision += 1;
        }
        if roster.preparing.is_some() || roster.built == roster.revision {
            return;
        }
        let revision = roster.revision;
        roster.preparing = Some(revision);
        // Checkouts first: an account's copy of one is the checkout's.
        let checkouts = self
            .projects_named()
            .filter(|repo| repo.remote.is_none())
            .filter_map(|repo| {
                Some(Input::Checkout {
                    info: Box::new(repo.snapshot.repo.clone()?),
                    name: repo.name(),
                    host: repo.host.clone(),
                })
            });
        let listings = self
            .repos
            .iter()
            .filter(|repo| repo.remote.is_some())
            .map(|repo| Input::Listing {
                items: repo.snapshot.items.clone(),
                history: repo.snapshot.history.clone(),
            });
        let inputs: Vec<Input> = checkouts.chain(listings).collect();
        let task = cx.background_executor().spawn(async move { build(inputs) });
        cx.spawn(async move |this, cx| {
            let entries = task.await;
            let _ = this.update(cx, |this, cx| this.repo_roster_built(revision, entries, cx));
        })
        .detach();
    }

    /// Takes a build; one for an older revision is dropped for the next.
    fn repo_roster_built(&mut self, revision: u64, entries: Vec<Entry>, cx: &mut Context<Self>) {
        let select = &mut self.repo_select;
        select.roster.preparing = None;
        if select.roster.revision != revision {
            self.prepare_repo_roster(cx);
            return;
        }
        select.roster.entries = entries;
        select.roster.built = revision;
        select.reflag();
        select.picker.refilter(select.roster.entries());
        cx.notify();
    }
}

/// Every repository `inputs` name, once, by name; flags are set after.
fn build(inputs: Vec<Input>) -> Vec<Entry> {
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    let mut add = |key: String, name: &str, host: &str| {
        if !seen.insert(key.clone()) {
            return;
        }
        entries.push(Entry {
            key,
            name: name.to_owned(),
            host: host.to_owned(),
            active: false,
            new: false,
        });
    };
    // Each source once before a key is made for it, not each item.
    let mut sources: HashSet<&SourceKey> = HashSet::new();
    for input in &inputs {
        match input {
            Input::Checkout { info, name, host } => match &info.remote {
                Some(remote) => add(
                    identity(&remote.source),
                    &remote.source.repository,
                    &remote.source.host,
                ),
                None => {
                    if let Some(source) = info.beads_source() {
                        add(source.canonical(), name, host);
                    }
                }
            },
            Input::Listing { items, history } => {
                let settled = history
                    .iter()
                    .filter_map(|status| status.sample.as_ref())
                    .flat_map(|sample| sample.records())
                    .map(|record| &record.item);
                for item in items.iter().chain(settled) {
                    let source = &item.key.source;
                    if sources.insert(source) {
                        add(identity(source), &source.repository, &source.host);
                    }
                }
            }
        }
    }
    entries.sort_by_cached_key(|entry| (entry.name.to_lowercase(), entry.key.clone()));
    entries
}
