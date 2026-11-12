use super::*;
use control::{ControlCommand, FilesCommand, Invocation, Switch};

fn parse(args: &[&str]) -> Result<Invocation, CliError> {
    LaunchOptions::parse(args.iter().copied()).map(|options| match options.mode {
        LaunchMode::Control(invocation) => invocation,
        mode => panic!("{args:?} parsed as {mode:?}"),
    })
}

#[test]
fn files_reports_switches_and_reveals_with_the_shared_options() {
    for args in [&["files"][..], &["files", "status"]] {
        assert_eq!(
            parse(args).unwrap().command,
            ControlCommand::Files(FilesCommand::Panel { switch: None })
        );
    }
    for (word, switch) in [
        ("show", Switch::On),
        ("hide", Switch::Off),
        ("toggle", Switch::Toggle),
    ] {
        assert_eq!(
            parse(&["files", word]).unwrap().command,
            ControlCommand::Files(FilesCommand::Panel {
                switch: Some(switch)
            })
        );
    }
    let reveal = parse(&["files", "reveal", "src/main.rs", "--json", "--window", "3"]).unwrap();
    assert_eq!(
        reveal.command,
        ControlCommand::Files(FilesCommand::Reveal {
            path: "src/main.rs".into()
        })
    );
    assert!(reveal.json);
    assert_eq!(reveal.window, Some(3));
}

#[test]
fn files_refuses_bad_arguments() {
    assert!(parse(&["files", "reveal"]).is_err());
}
