//! Where a click on Walkr's icon goes: the newest waiting page of every
//! counted host, and the workspace that shows its work.
use super::Waiting;
use crate::automations::Machine;
use chrono::{DateTime, Utc};

/// The newest page that waits for a pick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Page<'a> {
    pub(crate) created: Option<DateTime<Utc>>,
    pub(crate) id: &'a str,
    pub(crate) machine: &'a Machine,
    /// The `repo.name` its record names.
    pub(crate) repo: Option<&'a str>,
}

/// A workspace of the window, in sidebar order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Workspace {
    /// The workspace is the one its host shows.
    pub(crate) active: bool,
    /// The index of its endpoint in the window.
    pub(crate) endpoint: usize,
    /// A pane that holds the window's live run is in it.
    pub(crate) holds_run: bool,
    pub(crate) id: String,
    pub(crate) machine: Machine,
    /// The repository name of its checkout.
    pub(crate) repo: Option<String>,
}

/// The newest waiting page of every host, by when it was made.
pub(crate) fn newest(waiting: &Waiting) -> Option<Page<'_>> {
    waiting
        .hosts()
        .iter()
        .filter_map(|host| {
            let tally = host.tally.as_ref()?;
            let newest = tally.newest.first()?;
            Some(Page {
                created: newest.created,
                id: &newest.id,
                machine: &host.machine,
                repo: tally.repo.as_deref(),
            })
        })
        .max_by(|a, b| (a.created, a.id).cmp(&(b.created, b.id)))
}

/// The workspace for `page`. The page of the live run, whose address names
/// `run_session`, goes to a workspace of its own host that holds one of its
/// panes. Any other
/// page goes to a workspace of its own host whose checkout repository has
/// the name its record gives. Of several, the one its host shows wins, else
/// the first in sidebar order. `None` when none matches.
pub(crate) fn place<'w>(
    page: &Page<'_>,
    run_session: Option<&str>,
    workspaces: &'w [Workspace],
) -> Option<&'w Workspace> {
    let of_run = page.machine.is_local() && run_session == Some(page.id);
    let holds_run: Vec<&Workspace> = workspaces
        .iter()
        .filter(|workspace| of_run && workspace.holds_run && workspace.machine == *page.machine)
        .collect();
    let candidates = if holds_run.is_empty() {
        let repo = page.repo.filter(|repo| !repo.is_empty())?;
        workspaces
            .iter()
            .filter(|workspace| {
                workspace.machine == *page.machine && workspace.repo.as_deref() == Some(repo)
            })
            .collect()
    } else {
        holds_run
    };
    candidates
        .iter()
        .find(|workspace| workspace.active)
        .or_else(|| candidates.first())
        .copied()
}
