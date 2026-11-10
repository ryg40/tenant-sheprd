//! The address a tab was asked for, kept beside the address its page is on.
//!
//! A site may answer the address it was asked for by sending the tab to
//! another site before it shows a page of its own, as a site behind a
//! sign-in does. The page the tab is on then is a step of that sign-in, and
//! its address works once: a tab restored there after a restart cannot
//! finish it. So such a tab keeps the address the user or an agent asked
//! for, and a restart loads that one, which starts the site's sign-in
//! afresh.
//!
//! The asked address counts only until the tab settles, which is the
//! earlier of two events:
//!
//! - A page of the asked site finishes loading. From then on a move to
//!   another site is the user's own, by a link, and a restart restores the
//!   last address as it does for every other tab. This is what tells a
//!   sign-in from a link: the sign-in takes the tab away before the asked
//!   site showed anything.
//! - The tab goes on from the place it was first sent to another address
//!   that is not on the asked site. The user is reading the other site
//!   then. A site that only moves to another host name, as from
//!   `example.com` to `www.example.com`, loses nothing either way: until
//!   the user goes on, its last address is where the asked one leads.
//!
//! What the rule does not cover: a site that shows a page of its own and
//! then sends the tab to its sign-in by script, a sign-in the site starts
//! later in the tab's life, and a sign-in with several pages at the other
//! site. Such tabs restore their last address, as all tabs did before.
//!
//! The asked address is kept without its query and fragment: they may hold
//! a token, and the file would keep it after the site moved the tab on.
use super::{Location, MAX_TABS, Place, Store, Tab, TabId, WebUrl};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    tab: TabId,
    url: WebUrl,
    /// Whether the tab settled, so the address no longer counts at a
    /// restart. Such an entry is not saved.
    #[serde(skip)]
    settled: bool,
}

/// The asked address of each tab that has one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct Asked(Vec<Entry>);

/// Whether `location` is on another origin than `asked`.
fn away(location: &Location, asked: &WebUrl) -> bool {
    !matches!(location, Location::Web { url } if url.origin() == asked.origin())
}

impl Asked {
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether a saved file's entries are within what the app keeps.
    pub(super) fn valid(&self) -> bool {
        self.0.len() <= MAX_TABS
    }

    /// Tab `id` was asked for `location`. Only a web address is kept, and
    /// only for the tabs in `tabs`: the entries of tabs that are gone go
    /// here, which bounds the list by the tabs the store keeps.
    fn ask(&mut self, id: TabId, location: &Location, tabs: &[Tab]) {
        self.0
            .retain(|entry| entry.tab != id && tabs.iter().any(|tab| tab.id == entry.tab));
        if let Location::Web { url } = location {
            self.0.push(Entry {
                tab: id,
                url: url.bare(),
                settled: false,
            });
        }
    }

    /// A load of tab `id` finished on `new`; the tab was on `old`. Settles
    /// the tab when a page of the asked site loaded, or when the tab went
    /// on from the place the site first sent it. Whether it settled now.
    pub(super) fn went(&mut self, id: TabId, old: Option<&Location>, new: &Location) -> bool {
        let pending = self
            .0
            .iter_mut()
            .find(|entry| entry.tab == id && !entry.settled);
        let Some(entry) = pending else {
            return false;
        };
        let arrived = !away(new, &entry.url);
        // A reload of the place it was sent to is not a step on.
        let went_on = old.is_some_and(|old| old != new && away(old, &entry.url));
        entry.settled = arrived || went_on;
        entry.settled
    }

    /// The address tab `id` was last asked for, settled or not.
    #[cfg(servo_pages)]
    fn get(&self, id: TabId) -> Option<&WebUrl> {
        self.0
            .iter()
            .find(|entry| entry.tab == id)
            .map(|entry| &entry.url)
    }

    /// The entries to save with `tabs`: those of tabs that still exist and
    /// have not settled.
    pub(super) fn of(&self, tabs: &[Tab]) -> Self {
        let kept = self
            .0
            .iter()
            .filter(|entry| !entry.settled && tabs.iter().any(|tab| tab.id == entry.tab))
            .cloned()
            .collect();
        Self(kept)
    }

    /// Puts each restored tab that a site sent away back on the address
    /// it was asked for, named after that address until its page reports
    /// a title.
    pub(super) fn restore(&self, tabs: &mut [Tab]) {
        for entry in &self.0 {
            let Some(tab) = tabs.iter_mut().find(|tab| tab.id == entry.tab) else {
                continue;
            };
            if tab.place != Place::Group
                || !matches!(&tab.location, Some(Location::Web { url }) if url.origin() != entry.url.origin())
            {
                continue;
            }
            let location = Location::Web {
                url: entry.url.clone(),
            };
            tab.title = super::clean_title(&location.default_title());
            tab.location = Some(location);
        }
    }
}

impl Store {
    /// Loads nothing: records that the user or an agent asked tab `id` for
    /// `location`, which is also where the tab is from now on. Returns
    /// whether the tab's address changed.
    pub(crate) fn asked(&mut self, id: TabId, location: Location) -> bool {
        let Some(tab) = self.get(id) else {
            return false;
        };
        let pages = tab.place == Place::Group;
        let changed = self.visited(id, Some(location.clone()), None);
        if pages {
            self.asked.ask(id, &location, &self.tabs);
            self.save();
        }
        changed
    }

    /// The address tab `id` was last asked for in this run, without its
    /// query: where to start again when the page it led to is of no use.
    /// A tab restored after it settled has none.
    #[cfg(servo_pages)]
    pub(crate) fn asked_address(&self, id: TabId) -> Option<&WebUrl> {
        self.get(id)?;
        self.asked.get(id)
    }

    /// Records the address a new tab was opened on as the one asked for.
    pub(super) fn opened(&mut self, id: TabId) {
        let Some(tab) = self.get(id).filter(|tab| tab.place == Place::Group) else {
            return;
        };
        if let Some(location @ Location::Web { .. }) = tab.location.clone() {
            self.asked.ask(id, &location, &self.tabs);
            self.save();
        }
    }
}
