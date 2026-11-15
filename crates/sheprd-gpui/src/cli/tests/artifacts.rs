use super::*;
use control::{ArtifactsCommand, ControlCommand, Invocation};

fn parse(args: &[&str]) -> Result<Invocation, CliError> {
    LaunchOptions::parse(args.iter().copied()).map(|options| match options.mode {
        LaunchMode::Control(invocation) => invocation,
        mode => panic!("{args:?} parsed as {mode:?}"),
    })
}

#[test]
fn artifacts_takes_status_start_off_and_open_with_the_shared_options() {
    for (args, command) in [
        (&["artifacts"][..], ArtifactsCommand::Status),
        (&["artifacts", "status"], ArtifactsCommand::Status),
        (&["artifacts", "start"], ArtifactsCommand::Start),
        (&["artifacts", "off"], ArtifactsCommand::Off),
        (&["artifacts", "stop"], ArtifactsCommand::Off),
        (&["artifacts", "open"], ArtifactsCommand::Open),
    ] {
        assert_eq!(
            parse(args).unwrap().command,
            ControlCommand::Artifacts(command),
            "{args:?}"
        );
    }
    assert_eq!(
        parse(&["artifacts", "start", "--json", "--window", "7"]).unwrap(),
        Invocation {
            command: ControlCommand::Artifacts(ArtifactsCommand::Start),
            json: true,
            window: Some(7),
        }
    );
    for invalid in [
        &["artifacts", "restart"][..],
        &["artifacts", "start", "--yes"],
        &["artifacts", "open", "extra"],
    ] {
        assert!(parse(invalid).is_err(), "{invalid:?}");
    }
    assert!(matches!(
        parse(&["artifacts", "--help"]).unwrap().command,
        ControlCommand::Help(Some("artifacts"))
    ));
}
