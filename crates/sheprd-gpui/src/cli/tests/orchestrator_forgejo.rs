//! `orchestrator issues --forgejo HOST` lists a Forgejo account's issues,
//! by the host as `FORGEJO_HOST` names it, and never a checkout's beside it.
use super::*;
use control::{ControlCommand, Invocation, Items, ItemsOf, OrchestratorCommand};

fn parse(args: &[&str]) -> Result<Invocation, CliError> {
    LaunchOptions::parse(args.iter().copied()).map(|options| match options.mode {
        LaunchMode::Control(invocation) => invocation,
        mode => panic!("{args:?} parsed as {mode:?}"),
    })
}

#[test]
fn an_account_is_named_by_its_host_before_or_after_the_verb() {
    for args in [
        &["orchestrator", "issues", "--forgejo", "Forgejo.Example.com"][..],
        &[
            "orchestrator",
            "--forgejo",
            "https://forgejo.example.com/",
            "issues",
        ],
    ] {
        assert_eq!(
            parse(args).unwrap().command,
            ControlCommand::Orchestrator(OrchestratorCommand::Items {
                of: ItemsOf::ForgejoAccount("forgejo.example.com".into()),
                items: Items::Issues,
            }),
            "{args:?}"
        );
    }
    let invocation = parse(&[
        "orchestrator",
        "items",
        "--forgejo",
        "git.example.com:8443",
        "--json",
    ])
    .unwrap();
    assert!(invocation.json);
    assert_eq!(
        invocation.command,
        ControlCommand::Orchestrator(OrchestratorCommand::Items {
            of: ItemsOf::ForgejoAccount("git.example.com:8443".into()),
            items: Items::All,
        })
    );
}

#[test]
fn an_account_is_not_opened_or_mixed_with_a_checkout() {
    let unexpected = CliError::Unexpected {
        command: "orchestrator",
        argument: "--forgejo".into(),
    };
    for args in [
        &["orchestrator", "--forgejo", "forgejo.example.com"][..],
        &["orchestrator", "open", "--forgejo", "forgejo.example.com"],
        &[
            "orchestrator",
            "issues",
            "--repo",
            "/src/app",
            "--forgejo",
            "forgejo.example.com",
        ],
    ] {
        assert_eq!(parse(args).unwrap_err(), unexpected, "{args:?}");
    }
    assert_eq!(
        parse(&["orchestrator", "issues", "--forgejo"]).unwrap_err(),
        CliError::MissingValue("--forgejo")
    );
    assert_eq!(
        parse(&["orchestrator", "issues", "--forgejo", "/"]).unwrap_err(),
        CliError::InvalidValue("--forgejo")
    );
}
