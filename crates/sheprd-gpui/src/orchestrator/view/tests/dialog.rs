//! The dispatch dialog fits short windows and large text by scrolling.
use super::*;
use crate::orchestrator::{
    Request, Timing,
    view::{Look, OrchestratorView},
};
use crate::teleport::AgentKind;
use gpui::{TestAppContext, px, size};
use std::sync::Arc;

#[gpui::test]
fn the_dispatch_dialog_stays_within_a_short_window(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let request = Request {
        target: sheprd_client::ConnectTarget::Local,
        checkout: dir.path().to_string_lossy().into_owned(),
        workspace_id: "w1".into(),
        token: None,
        data_root: Some(dir.path().join("data")),
        azure_store: Default::default(),
        gitlab_store: Default::default(),
        timing: Timing::default(),
    };
    let config = crate::config::Config::default();
    let mut ui = config.ui.clone();
    ui.size = 48.;
    let look = Look {
        theme: crate::config::Theme::default(),
        ui,
        mono: config.terminal.clone(),
    };
    let (view, cx) = cx.add_window_view(|_, cx| OrchestratorView::with_repo(request, look, cx));
    cx.simulate_resize(size(px(640.), px(400.)));
    let issue = item(Provider::Github, "377", "Icons", "2026-10-04T00:00:00Z");
    let key = issue.key.canonical();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.repo_mut().snapshot.items = Arc::new(vec![issue]);
            view.refresh_rows();
            view.open_dispatch(key, window, cx);
        })
    });
    cx.run_until_parked();
    let card = cx
        .debug_bounds("orchestrator-overlay-card")
        .unwrap_or_else(|| panic!("the dialog's card"));
    assert!(
        card.top() >= px(0.) && card.bottom() <= px(400.),
        "{card:?}"
    );
}

/// The folded fields are not drawn, so the dialog must not open with focus
/// on one: keys would then miss the dialog's own Enter and Escape.
#[gpui::test]
fn the_dispatch_dialog_takes_escape_and_enter_as_it_opens(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let request = Request {
        target: sheprd_client::ConnectTarget::Local,
        checkout: dir.path().to_string_lossy().into_owned(),
        workspace_id: "w1".into(),
        token: None,
        data_root: Some(dir.path().join("data")),
        azure_store: Default::default(),
        gitlab_store: Default::default(),
        timing: Timing::default(),
    };
    let config = crate::config::Config::default();
    let look = Look {
        theme: crate::config::Theme::default(),
        ui: config.ui.clone(),
        mono: config.terminal.clone(),
    };
    let (view, cx) = cx.add_window_view(|_, cx| OrchestratorView::with_repo(request, look, cx));
    let issue = item(Provider::Github, "377", "Icons", "2026-10-04T00:00:00Z");
    let key = issue.key.canonical();
    // The start goes to this channel, not to a worker that runs `herdr`.
    let (actions, started) = std::sync::mpsc::sync_channel(1);
    let open = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.action_sink = Some(actions.clone());
                view.repo_mut().snapshot.items = Arc::new(vec![issue.clone()]);
                view.refresh_rows();
                view.open_dispatch(key.clone(), window, cx);
                view.repo_mut().snapshot.installed = Some(Arc::new(vec![AgentKind::Claude]));
                view.reconcile_agents();
            })
        });
        cx.run_until_parked();
    };
    open(cx);
    cx.simulate_keystrokes("escape");
    assert!(view.read_with(cx, |view, _| view.dialog.is_none()));
    // Enter starts the agent, which closes the dialog.
    open(cx);
    cx.simulate_keystrokes("enter");
    assert!(view.read_with(cx, |view, _| view.dialog.is_none()));
    assert!(matches!(
        started.try_recv(),
        Ok(crate::orchestrator::Action::Dispatch(_))
    ));
}
