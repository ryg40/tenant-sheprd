//! Where a click on Walkr's icon goes: the newest waiting page wins, a
//! Compare session goes to a workspace of its own host whose repository
//! matches, and with no match the section shows with no workspace change.
use crate::{
    automations::Machine,
    walkr::{
        history::pending::{Pending, Tally},
        waiting::{
            RemoteState, Waiting,
            target::{Workspace, newest, place},
        },
    },
};
use chrono::{TimeZone as _, Utc};

fn pending(id: &str, hour: u32) -> Pending {
    Pending {
        id: id.to_owned(),
        created: Utc.with_ymd_and_hms(2026, 11, 1, hour, 0, 0).single(),
        title: None,
    }
}

fn tally(pages: Vec<Pending>, repo: &str) -> Tally {
    Tally {
        waiting: pages.len(),
        newest: pages,
        repo: Some(repo.to_owned()),
        sessions: 0,
    }
}

fn workspace(endpoint: usize, id: &str, machine: Machine, repo: &str) -> Workspace {
    Workspace {
        active: false,
        endpoint,
        holds_run: false,
        id: id.to_owned(),
        machine,
        repo: Some(repo.to_owned()),
    }
}

fn ssh() -> Machine {
    Machine::Ssh("ssh-host.example.test".to_owned())
}

#[test]
fn the_newest_waiting_page_of_any_host_wins() {
    let mut waiting = Waiting::default();
    waiting.test_answered(
        Machine::Local,
        "This computer",
        tally(
            vec![pending("20261101T090000Z-000000000001", 9)],
            "local-repo",
        ),
    );
    waiting.test_answered(
        ssh(),
        "ssh-host.example.test",
        tally(
            vec![
                pending("20261101T110000Z-000000000002", 11),
                pending("20261101T080000Z-000000000003", 8),
            ],
            "valuetainme",
        ),
    );
    let page = newest(&waiting).unwrap();
    assert_eq!(page.id, "20261101T110000Z-000000000002");
    assert_eq!(page.machine, &ssh());
    assert_eq!(page.repo, Some("valuetainme"));
}

#[test]
fn a_session_goes_to_a_matching_workspace_of_its_own_host_only() {
    let mut waiting = Waiting::default();
    waiting.test_answered(
        ssh(),
        "ssh-host.example.test",
        tally(
            vec![pending("20261101T110000Z-000000000002", 11)],
            "valuetainme",
        ),
    );
    let page = newest(&waiting).unwrap();
    // The same repository on this computer is another host's workspace.
    let mut workspaces = vec![
        workspace(0, "local-1", Machine::Local, "valuetainme"),
        workspace(1, "remote-1", ssh(), "other"),
        workspace(1, "remote-2", ssh(), "valuetainme"),
        workspace(1, "remote-3", ssh(), "valuetainme"),
    ];
    assert_eq!(place(&page, None, &workspaces).unwrap().id, "remote-2");
    // Of several matches, the one its host shows.
    workspaces[3].active = true;
    assert_eq!(place(&page, None, &workspaces).unwrap().id, "remote-3");
    // Only another host's workspace matches: none.
    workspaces.truncate(2);
    assert!(place(&page, None, &workspaces).is_none());
}

#[test]
fn the_page_of_the_live_run_goes_to_a_workspace_that_holds_it() {
    let id = "20261101T110000Z-000000000002";
    let mut waiting = Waiting::default();
    waiting.test_answered(
        Machine::Local,
        "This computer",
        tally(vec![pending(id, 11)], "valuetainme"),
    );
    let page = newest(&waiting).unwrap();
    let mut workspaces = vec![
        workspace(0, "by-repo", Machine::Local, "valuetainme"),
        workspace(0, "holds-run", Machine::Local, "elsewhere"),
    ];
    workspaces[1].holds_run = true;
    assert_eq!(place(&page, Some(id), &workspaces).unwrap().id, "holds-run");
    // Another session than the run's goes by its repository.
    assert_eq!(
        place(&page, Some("20261101T120000Z-000000000009"), &workspaces)
            .unwrap()
            .id,
        "by-repo"
    );
}

/// A pane ID of the run on another host is not the run's pane: a local
/// page never goes to an SSH host's workspace.
#[test]
fn the_page_of_the_live_run_never_goes_to_another_host() {
    let id = "20261101T110000Z-000000000002";
    let mut waiting = Waiting::default();
    waiting.test_answered(
        Machine::Local,
        "This computer",
        tally(vec![pending(id, 11)], "valuetainme"),
    );
    let page = newest(&waiting).unwrap();
    let mut workspaces = vec![workspace(1, "same-pane-id", ssh(), "valuetainme")];
    workspaces[0].holds_run = true;
    assert!(place(&page, Some(id), &workspaces).is_none());
}

#[gpui::test]
fn no_match_shows_the_section_and_keeps_the_workspace(cx: &mut gpui::TestAppContext) {
    use crate::sidebar::{ActivityPanel, layout_tests::fixture_window};
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.config.sidebar_style.activity_bar = true;
        view
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.walkr.waiting.test_answered(
                ssh(),
                "ssh-host.example.test",
                tally(
                    vec![pending("20261101T110000Z-000000000002", 11)],
                    "valuetainme",
                ),
            );
            let endpoint = view.selected_endpoint;
            let focused = view
                .live
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.focused_workspace_id.clone());
            view.click_walkr(window, cx);
            assert_eq!(view.activity_section(), Some(ActivityPanel::Walkr));
            assert!(view.walkr.waiting.unmatched);
            assert_eq!(view.selected_endpoint, endpoint);
            assert_eq!(
                view.live
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.focused_workspace_id.clone()),
                focused
            );
        });
    });
}

/// A click on Walkr's icon, with a session of this computer that waits and
/// no run connected, asks this computer's Walkr server for the page. Here
/// the attempt fails (no server record in the test's home, a build without
/// pages, or Windows), so the session stays picked and its text shows.
#[gpui::test]
fn a_waiting_local_session_with_no_run_asks_for_its_page(cx: &mut gpui::TestAppContext) {
    use crate::sidebar::layout_tests::fixture_window;
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.config.sidebar_style.activity_bar = true;
        view
    });
    let home = tempfile::tempdir().unwrap();
    let id = "20261101T120000Z-000000000003";
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.walkr.waiting.test_answered(
                Machine::Local,
                "This computer",
                tally(vec![pending(id, 12)], "valuetainme"),
            );
            view.walkr.waiting.test_home(&Machine::Local, home.path());
            view.click_walkr(window, cx);
            let remote = view.walkr.waiting.remote.as_ref().unwrap();
            assert!(remote.machine.is_local() && remote.id == id);
        });
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            let state = &view.walkr.waiting.remote.as_ref().unwrap().state;
            #[cfg(windows)]
            assert!(
                matches!(
                    state,
                    RemoteState::Failed(crate::walkr::waiting::Error::LocalUnsupported)
                ),
                "{state:?}"
            );
            #[cfg(not(windows))]
            assert!(matches!(state, RemoteState::Failed(_)), "{state:?}");
            assert_eq!(view.walkr.sessions.pick(), Some(id));
            assert!(view.walkr.detail_shown());
        });
    });
}
