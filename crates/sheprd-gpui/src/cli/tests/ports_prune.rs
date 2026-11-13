//! `ports prune`: only `--json` and `--window` ride along, and nothing
//! that would skip the app's confirmation parses.
use super::*;
use control::{ControlCommand, PortsCommand};

#[test]
fn ports_prune_takes_json_and_window() {
    let mode = LaunchOptions::parse(["ports", "prune", "--json", "--window", "3"])
        .unwrap()
        .mode;
    assert!(matches!(
        mode,
        LaunchMode::Control(control::Invocation {
            command: ControlCommand::Ports(PortsCommand::Prune),
            window: Some(3),
            json: true,
        })
    ));
    let mode = LaunchOptions::parse(["ports", "prune"]).unwrap().mode;
    assert!(matches!(
        mode,
        LaunchMode::Control(control::Invocation {
            command: ControlCommand::Ports(PortsCommand::Prune),
            window: None,
            json: false,
        })
    ));
}

#[test]
fn ports_prune_refuses_anything_else_before_startup() {
    for extra in ["--yes", "--force", "--confirm", "--all", "3000", "--window"] {
        assert!(
            LaunchOptions::parse(["ports", "prune", extra]).is_err(),
            "{extra}"
        );
    }
}
