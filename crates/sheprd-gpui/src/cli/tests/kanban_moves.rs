use super::*;

#[test]
fn kanban_moves_only_parse_proposals_and_refuse_approval_or_transfer_options() {
    let item = "forgejo:forgejo.example.test:team/app:7";
    let lane = "forgejo:forgejo.example.test:team/app";
    let base = [
        "orchestrator",
        "board",
        "move",
        item,
        "--lane",
        lane,
        "--to",
        "done",
    ];
    for forbidden in [
        vec!["--yes"],
        vec!["--force"],
        vec!["--confirm"],
        vec!["--destination-lane", lane],
        vec!["--repo", "/synthetic"],
        vec!["--forgejo", "forgejo.example.test"],
        vec!["--before", lane],
    ] {
        assert!(LaunchOptions::parse(base.into_iter().chain(forbidden)).is_err());
    }
    let parsed = LaunchOptions::parse(base.into_iter().chain(["--json", "--window", "7"])).unwrap();
    assert!(
        matches!(parsed.mode, LaunchMode::Control(control::Invocation {
        command: control::ControlCommand::Orchestrator(control::OrchestratorCommand::BoardMove {
            item: key,
            lane: source,
            to: sheprd_control_protocol::BoardColumn::Done,
        }),
        json: true,
        window: Some(7),
    }) if key == item && source == lane)
    );
    use sheprd_control_protocol::protocol::orchestrator::OrchestratorOp;
    for value in [
        serde_json::json!({"op":"board_move", "item":item, "lane":lane, "to":"done", "yes":true}),
        serde_json::json!({"op":"board_move", "item":item, "lane":lane, "to":"done", "force":true}),
        serde_json::json!({"op":"board_confirm", "item":item}),
        serde_json::json!({"op":"board_write", "item":item}),
    ] {
        assert!(serde_json::from_value::<OrchestratorOp>(value).is_err());
    }
}
