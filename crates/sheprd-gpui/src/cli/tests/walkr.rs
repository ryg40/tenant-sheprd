use super::*;
use control::{ControlCommand, Invocation, RunSource, WalkrCommand};
use sheprd_control_protocol::protocol::walkr::RunAddress;

const ADDRESS: &str =
    "http://127.0.0.1:4100/_walkr/compare/g/INVENTEDSECRET#20261101T120000Z-0123456789ab";

fn parse(args: &[&str]) -> Result<Invocation, CliError> {
    LaunchOptions::parse(args.iter().copied()).map(|options| match options.mode {
        LaunchMode::Control(invocation) => invocation,
        mode => panic!("{args:?} parsed as {mode:?}"),
    })
}

fn walkr(command: WalkrCommand) -> Invocation {
    Invocation {
        command: ControlCommand::Walkr(command),
        json: false,
        window: None,
    }
}

#[test]
fn walkr_connect_takes_an_address_or_standard_input_and_show() {
    let address = || RunSource::Argument(RunAddress::new(ADDRESS));
    for (args, address, show) in [
        (vec!["walkr", "connect", ADDRESS], address(), false),
        (vec!["walkr", "connect", ADDRESS, "--show"], address(), true),
        (vec!["walkr", "connect", "--show", ADDRESS], address(), true),
        (vec!["walkr", "connect", "-"], RunSource::Stdin, false),
        (
            vec!["walkr", "connect", "-", "--show"],
            RunSource::Stdin,
            true,
        ),
    ] {
        assert_eq!(
            parse(&args).unwrap(),
            walkr(WalkrCommand::Connect { address, show })
        );
    }
    assert_eq!(
        parse(&["walkr", "connect", "-", "--json", "--window", "7"]).unwrap(),
        Invocation {
            command: ControlCommand::Walkr(WalkrCommand::Connect {
                address: RunSource::Stdin,
                show: false,
            }),
            json: true,
            window: Some(7),
        }
    );
    // The client parses nothing of the address: the app refuses a bad one.
    assert!(parse(&["walkr", "connect", "not an address"]).is_ok());
    assert!(parse(&["walkr", "connect"]).is_err());
    assert!(parse(&["walkr", "connect", "--show"]).is_err());
    assert!(parse(&["walkr", "connect", ADDRESS, "extra"]).is_err());
}

#[test]
fn a_parsed_connect_and_its_errors_show_host_and_port_only() {
    let parsed = parse(&["walkr", "connect", ADDRESS, "--show"]).unwrap();
    let shown = format!("{parsed:?}");
    assert!(shown.contains("127.0.0.1:4100"), "{shown}");
    assert!(!shown.contains("INVENTEDSECRET"), "{shown}");
    let refused = parse(&["walkr", "connect", ADDRESS, "--close"]).unwrap_err();
    let shown = format!("{refused} {refused:?}");
    assert!(!shown.contains("INVENTEDSECRET"), "{shown}");
}

#[test]
fn a_misplaced_address_is_in_no_error_as_text_json_or_debug() {
    let other = "http://127.0.0.1:4100/_walkr/compare/g/INVENTEDOTHER";
    for args in [
        // The reviewer's two: standard input with an argument, and two.
        vec!["walkr", "connect", "-", ADDRESS],
        vec!["walkr", "connect", ADDRESS, other, "--json"],
        vec!["walkr", "connect", ADDRESS, "-"],
        vec!["walkr", "connect", "--show", ADDRESS, "--show", other],
        // After `--`, and inside or after an option.
        vec!["walkr", "connect", "-", "--", ADDRESS],
        vec!["walkr", "connect", "--", ADDRESS],
        vec![
            "walkr",
            "connect",
            "-",
            "--url=http://127.0.0.1:4100/INVENTEDSECRET",
        ],
        vec!["walkr", "connect", "--window", ADDRESS],
        vec!["walkr", "connect", "--window", "1", "--window", ADDRESS],
        vec!["walkr", "connect", "-", "--window", "1", ADDRESS],
        // In place of the subcommand, and after another one.
        vec!["walkr", ADDRESS],
        vec!["walkr", ADDRESS, "connect"],
        vec!["walkr", "--json", ADDRESS],
        vec!["walkr", "disconnect", ADDRESS],
        vec!["walkr", "status", ADDRESS, "--json"],
    ] {
        let error = parse(&args).unwrap_err();
        let shown = format!("{error}\n{}\n{error:?}", error.json());
        for hidden in ["INVENTED", "4100", "_walkr", "compare", "20261101", "http"] {
            assert!(!shown.contains(hidden), "{args:?}: {shown}");
        }
    }
    // A mistyped subcommand is no address, and is still named.
    let typo = parse(&["walkr", "conect", ADDRESS]).unwrap_err();
    assert!(typo.to_string().contains("conect"), "{typo}");
    assert!(!format!("{typo} {typo:?}").contains("INVENTED"), "{typo:?}");
}

#[test]
fn walkr_disconnect_status_and_clear_take_no_argument() {
    assert_eq!(
        parse(&["walkr", "disconnect"]).unwrap(),
        walkr(WalkrCommand::Disconnect)
    );
    assert_eq!(
        parse(&["walkr", "status"]).unwrap(),
        walkr(WalkrCommand::Status)
    );
    assert_eq!(
        parse(&["walkr", "clear"]).unwrap(),
        walkr(WalkrCommand::Clear)
    );
    assert!(parse(&["walkr", "clear", "--json"]).unwrap().json);
    assert!(parse(&["walkr", "clear", "local"]).is_err());
    assert!(parse(&["walkr", "disconnect", "--json"]).unwrap().json);
    assert!(parse(&["walkr", "disconnect", "extra"]).is_err());
    // No command closes the page.
    assert!(parse(&["walkr", "close"]).is_err());
    assert!(parse(&["walkr"]).is_err());
}
