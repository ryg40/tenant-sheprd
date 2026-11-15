//! A dialog or confirmation opened on a repository the selection then
//! hides sends nothing when confirmed: no dispatch, stop, or removal
//! reaches the worker and no move reaches the tracker, also after the new
//! board settles.
use super::*;
use crate::{
    orchestrator::{
        Request, Timing,
        view::{RepoSpec, dispatch::Confirm, repo::Repo, tests::kanban_moves::Peer},
    },
    teleport::AgentKind,
};
use sheprd_control_protocol::BoardColumn;
use sheprd_orchestrator::kanban::identity::LaneKey;
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

/// The one bound on a wait for the move worker; reached only on failure.
const WORKER: Duration = Duration::from_secs(30);

#[gpui::test]
fn a_hidden_repositorys_dialogs_send_nothing_when_confirmed(cx: &mut gpui::TestAppContext) {
    let (dir, view, cx) = view(cx);
    let app = issue("Team/App", 1);
    let item = app.key.canonical();
    let hide_app = || Choice::Only(vec![key("team/web")]);
    let (actions, sent) = mpsc::sync_channel(4);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.action_sink = Some(actions);
            view.repo_mut().snapshot.installed = Some(Arc::new(vec![AgentKind::Claude]));
            view.open_dispatch(item.clone(), window, cx);
            assert!(view.dialog.is_some(), "the dispatch dialog opened");
            view.choose_repos(hide_app(), cx).unwrap();
            view.dialog_key("enter", window, cx);
        })
    });
    cx.run_until_parked();
    assert!(sent.try_recv().is_err(), "a hidden item was dispatched");

    // A stop or a removal asked on the item's page, confirmed by the
    // button drawn before the repository was hidden.
    let repo = view.update(cx, |view, _| view.repo_mut().key.clone());
    let asked = [
        Confirm::Stop {
            run: "r1".into(),
            agent: "claude".into(),
            item: item.clone(),
            repo: repo.clone(),
        },
        Confirm::Remove {
            run: "r1".into(),
            branch: "agent/1".into(),
            item: item.clone(),
            repo,
        },
    ];
    for confirm in asked {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.choose_repos(Choice::All, cx).unwrap();
                view.ask(confirm.clone(), cx);
                view.choose_repos(hide_app(), cx).unwrap();
                view.confirm_overlay(confirm, window, cx);
            })
        });
        cx.run_until_parked();
        assert!(sent.try_recv().is_err(), "a hidden item's run was acted on");
    }

    // A Beads task two checkouts share: hiding the one its page acts
    // through stops the action, though the other still shows the task.
    let task = shared_task(&view, dir.path(), cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.choose_repos(Choice::All, cx).unwrap();
            view.active = 0;
            let confirm = Confirm::Stop {
                run: "r2".into(),
                agent: "claude".into(),
                item: task.clone(),
                repo: view.repo_mut().key.clone(),
            };
            view.ask(confirm.clone(), cx);
            view.choose_repos(Choice::Only(vec![key("Team/Lib")]), cx)
                .unwrap();
            assert!(
                !view.item_hidden(&task),
                "the other checkout shows the task"
            );
            view.confirm_overlay(confirm, window, cx);
        })
    });
    cx.run_until_parked();
    assert!(
        sent.try_recv().is_err(),
        "a hidden checkout's run was stopped"
    );
    view.update(cx, |view, cx| {
        view.repos.truncate(2);
        view.refresh_rows();
        view.prepare_repo_roster(cx);
    });
    cx.run_until_parked();

    let (signal, prepared) = mpsc::channel();
    let peer = Peer {
        signal: Some(signal),
        ..Peer::default()
    };
    let lane = LaneKey::try_from(&app.key.source).unwrap();
    view.update(cx, |view, cx| {
        view.choose_repos(Choice::All, cx).unwrap();
        view.fixture_move(peer.clone());
        view.show_kanban(true, cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.propose_card(&item, &lane, BoardColumn::InProgress, window, cx)
                .unwrap();
        })
    });
    assert_eq!(
        prepared.recv_timeout(WORKER),
        Ok(()),
        "the worker reached the move"
    );
    // The dialog must offer its confirmation first, or a refusal would
    // prove nothing. The worker stores its plan on its own thread just
    // after the signal, so the test polls the dialog on its executor.
    let end = Instant::now() + WORKER;
    while !view.update(cx, |view, cx| {
        view.poll_move_dialog(cx);
        view.move_confirmable()
    }) {
        assert!(Instant::now() < end, "the move was never ready to confirm");
        cx.run_until_parked();
    }
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.choose_repos(hide_app(), cx).unwrap();
            view.confirm_move_for_test(window, cx);
        })
    });
    // Once the board without the lane settles, confirming still writes nothing.
    cx.run_until_parked();
    cx.update(|window, cx| view.update(cx, |view, cx| view.confirm_move_for_test(window, cx)));
    cx.run_until_parked();
    // Every copy of the peer is gone once no worker holds one, so a write
    // a submitted move made would be recorded by then.
    let writes = peer.writes.clone();
    drop(peer);
    view.update(cx, |view, _| view.fixture_move(Peer::default()));
    assert_eq!(
        prepared.recv_timeout(WORKER),
        Err(mpsc::RecvTimeoutError::Disconnected),
        "a worker still holds the peer"
    );
    assert!(writes.lock().unwrap().is_empty(), "a hidden card moved");
    assert!(sent.try_recv().is_err());
}

/// Adds a second checkout, of `Team/Lib`, whose Beads database is the one
/// the `Team/App` checkout uses, so both list the same task.
fn shared_task(
    view: &Entity<OrchestratorView>,
    dir: &std::path::Path,
    cx: &mut VisualTestContext,
) -> String {
    let source = SourceKey {
        provider: Provider::Beads,
        host: "local".into(),
        repository: "/tmp/app".into(),
    };
    let mut task = item(
        Provider::Beads,
        "bd-1",
        "Shared task",
        "2026-10-03T00:00:00Z",
    );
    task.key.source = source;
    let key = task.key.canonical();
    let request = Request {
        target: sheprd_client::ConnectTarget::Local,
        checkout: dir.join("lib").to_string_lossy().into_owned(),
        workspace_id: "lib".into(),
        token: None,
        data_root: Some(dir.join("lib-data")),
        azure_store: Default::default(),
        gitlab_store: Default::default(),
        timing: Timing::default(),
    };
    let mut lib = Repo::start(RepoSpec {
        key: "lib".into(),
        host: "local".into(),
        request,
    });
    lib.snapshot.repo = Some(RepoInfo {
        repository: crate::orchestrator::Repository::Local("/tmp/lib/.git".into()),
        main_root: "/tmp/lib".into(),
        beads: true,
        beads_root: Some("/tmp/app".into()),
        remote: Some(Remote {
            name: "origin".into(),
            url: format!("git@{HOST}:Team/Lib.git"),
            source: forgejo("Team/Lib"),
        }),
    });
    lib.snapshot.items = Arc::new(vec![task.clone()]);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let app = Arc::make_mut(&mut view.repos[0].snapshot.items);
            app.push(task);
            view.repos.push(lib);
            view.refresh_rows();
            view.prepare_repo_roster(cx);
        })
    });
    cx.run_until_parked();
    key
}
