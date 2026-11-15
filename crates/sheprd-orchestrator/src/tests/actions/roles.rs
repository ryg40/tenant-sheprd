use super::*;
use crate::{actions::agent_options, role::Role};

#[test]
fn a_role_adds_the_agents_own_prompt_option_after_the_model() {
    let file = Some("/home/u/coordinator.md");
    assert_eq!(
        agent_options(AgentKind::Claude, None, file),
        [
            "--",
            "--append-system-prompt-file",
            "/home/u/coordinator.md"
        ]
    );
    assert_eq!(
        agent_options(AgentKind::Claude, Some("claude-opus-5-5"), file),
        [
            "--",
            "--model",
            "claude-opus-5-5",
            "--append-system-prompt-file",
            "/home/u/coordinator.md"
        ]
    );
    assert_eq!(
        agent_options(AgentKind::Pi, None, file),
        ["--", "--append-system-prompt", "/home/u/coordinator.md"]
    );
    assert_eq!(
        agent_options(AgentKind::Pi, Some("sol"), file),
        [
            "--",
            "--model",
            "sol",
            "--append-system-prompt",
            "/home/u/coordinator.md"
        ]
    );
    // The agent's default role starts it as before.
    assert!(agent_options(AgentKind::Claude, None, None).is_empty());
    assert_eq!(
        agent_options(AgentKind::Pi, Some("sol"), None),
        ["--", "--model", "sol"]
    );
}

#[test]
fn a_missing_prompt_file_refuses_the_dispatch_before_any_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    let missing = dir.path().join("no-such-role.md");
    let mut with_role = request();
    with_role.role = Some(Role {
        name: "Planning agent".into(),
        agents: vec!["claude".into(), "pi".into()],
        prompt_file: missing.to_string_lossy().into_owned(),
    });
    let error = perform(
        &site,
        &Action::Dispatch(Box::new(with_role)),
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(
        matches!(error, Error::Role(role::Error::Missing { .. })),
        "{error:?}"
    );
    assert!(calls(dir.path()).is_empty(), "Herdr was never called");
    assert!(
        !site.database.exists()
            || Store::open(&site.database)
                .unwrap()
                .runs()
                .unwrap()
                .is_empty(),
        "no run was recorded"
    );
}
