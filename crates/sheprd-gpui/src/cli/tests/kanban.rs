use super::*;

#[test]
fn kanban_board_rejects_selectors_and_oversized_cursors_before_startup() {
    for args in [
        vec!["orchestrator", "board", "--repo", "/synthetic"],
        vec!["orchestrator", "board", "--forgejo", "forgejo.example.test"],
        vec!["orchestrator", "board", "--yes"],
    ] {
        assert!(LaunchOptions::parse(args).is_err());
    }
    let cursor = "x".repeat(sheprd_control_protocol::protocol::orchestrator::MAX_CURSOR_BYTES + 1);
    assert!(LaunchOptions::parse(["orchestrator", "board", "--cursor", &cursor]).is_err());
    let mode = LaunchOptions::parse(["orchestrator", "board", "--window", "7", "--json"])
        .unwrap()
        .mode;
    assert!(matches!(
        mode,
        LaunchMode::Control(control::Invocation {
            command: control::ControlCommand::Orchestrator(control::OrchestratorCommand::Board {
                cursor: None
            }),
            window: Some(7),
            json: true,
        })
    ));
}
