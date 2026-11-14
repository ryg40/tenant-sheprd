//! What a window does with the waiting pages: it names the hosts to count,
//! takes their answers on its tick, and goes to the newest page on a click
//! of Walkr's icon.
use super::target::{self, Workspace};
use crate::{
    SheprdWindow,
    automations::Machine,
    sidebar::{ActivityPanel, workspace_entries},
};
use gpui::{Context, Window};
use sheprd_client::protocol::ClientShellWorkspace;
use std::{path::Path, time::Instant};

/// The repository name of a workspace's checkout: the label the daemon
/// gives its worktree, as Dispatch reads it, else the folder of a Git
/// checkout (one on a branch) that the daemon gives no worktree.
fn repository(workspace: &ClientShellWorkspace) -> Option<String> {
    if let Some(worktree) = &workspace.worktree {
        return Some(worktree.label.clone());
    }
    workspace.branch.as_ref()?;
    let folder = workspace.new_workspace_cwd.trim_end_matches('/');
    let name = folder.rsplit('/').next().filter(|name| !name.is_empty())?;
    Some(name.to_owned())
}

impl SheprdWindow {
    /// Starts a read of every counted host when one is due, and takes the
    /// answers. Whether the count or a failure changed.
    pub(crate) fn update_walkr_waiting(&mut self) -> bool {
        let now = Instant::now();
        if self.walkr.waiting.due(now) {
            let hosts = self
                .automation_hosts()
                .into_iter()
                .filter(|wanted| matches!(wanted.machine, Machine::Local | Machine::Ssh(_)))
                .map(|wanted| (wanted.machine, wanted.label, wanted.host))
                .collect();
            self.walkr.waiting.set_hosts(hosts);
            self.walkr.waiting.ask(now);
        }
        self.walkr.waiting.take_answers()
    }

    /// The window's workspaces on the hosts it can count, in sidebar order:
    /// the hosts as the sidebar lists them, and each host's workspaces with
    /// a repository's linked worktrees after its checkout.
    pub(crate) fn walkr_workspaces(&self) -> Vec<Workspace> {
        let run = self.walkr.run().filter(|run| run.live());
        let mut workspaces = Vec::new();
        for (index, endpoint) in self.displayed_devices() {
            let Some(machine) = Machine::of(&endpoint.connection.target) else {
                continue;
            };
            let Some(snapshot) = self.endpoint_snapshot(index).filter(|_| endpoint.enabled) else {
                continue;
            };
            let socket = endpoint.connection.target.socket_path().ok();
            let selected = index == self.selected_endpoint;
            // The run's panes on this daemon. A run is on this computer, so
            // only a local daemon holds them; a pane that names no daemon is
            // in the shown one, as `forget_walkr_panes` reads it.
            let run_panes: Vec<&str> = run
                .filter(|_| machine.is_local())
                .map(|run| {
                    run.panes()
                        .iter()
                        .filter(|pane| match &pane.daemon {
                            Some(daemon) => socket.as_deref() == Some(Path::new(daemon)),
                            None => selected,
                        })
                        .map(|pane| pane.id.as_str())
                        .collect()
                })
                .unwrap_or_default();
            for (at, _) in workspace_entries(&snapshot.workspaces) {
                let Some(workspace) = snapshot.workspaces.get(at) else {
                    continue;
                };
                let id = &workspace.workspace_id;
                workspaces.push(Workspace {
                    active: snapshot.focused_workspace_id.as_ref() == Some(id),
                    endpoint: index,
                    holds_run: snapshot.panes.iter().any(|pane| {
                        pane.workspace_id == *id && run_panes.contains(&pane.pane_id.as_str())
                    }),
                    id: id.clone(),
                    machine: machine.clone(),
                    repo: repository(workspace),
                });
            }
        }
        workspaces
    }

    /// A click on Walkr's icon. With a page that waits, it goes to the
    /// newest one: its workspace, the section, and the page. Without one,
    /// it shows the section as any icon does.
    pub(crate) fn click_walkr(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let workspaces = self.walkr_workspaces();
        let run_session = self
            .walkr
            .run()
            .filter(|run| run.live())
            .and_then(|run| run.session());
        let picked = target::newest(&self.walkr.waiting).map(|page| {
            let place = target::place(&page, run_session, &workspaces)
                .map(|workspace| (workspace.endpoint, workspace.id.clone()));
            (page.machine.clone(), page.id.to_owned(), place)
        });
        let Some((machine, id, place)) = picked.filter(|_| self.walkr.waiting.total() > 0) else {
            self.walkr.waiting.unmatched = false;
            self.select_activity_panel(ActivityPanel::Walkr, window, cx);
            return;
        };
        self.walkr.waiting.unmatched = place.is_none();
        if let Some((endpoint, workspace)) = &place {
            self.go_to_workspace(*endpoint, workspace, cx);
        }
        self.show_activity_panel(ActivityPanel::Walkr, window, cx);
        if machine.is_local() {
            // As a click of its row does: the page of the live run, else
            // the page of this computer's Walkr server.
            self.pick_local_walkr_session(id, true, window, cx);
        } else {
            self.walkr.sessions.back();
            self.open_waiting_walkr_page(machine, id, window, cx);
        }
        cx.notify();
    }
}
