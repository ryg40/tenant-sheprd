use super::*;

#[test]
fn browser_clear_data_takes_no_argument() {
    let mode = |args: &[&str]| LaunchOptions::parse(args.iter().copied()).map(|o| o.mode);
    assert_eq!(
        mode(&["browser", "clear-data"]).unwrap(),
        browser(BrowserCommand::ClearData)
    );
    // No flag confirms from the command line: the user confirms in the app.
    for extra in ["--yes", "--force", "all"] {
        assert_eq!(
            mode(&["browser", "clear-data", extra]).unwrap_err(),
            CliError::UnexpectedArgument(extra.into()),
            "{extra}"
        );
    }
}
