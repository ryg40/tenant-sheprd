//! `walkr status`, `walkr connect`, and `walkr disconnect` act on the
//! window of the caller's workspace.
use super::{Args, CliError, unknown};
use sheprd_control_protocol::protocol::walkr::RunAddress;

/// Where `walkr connect` takes the address from. The client parses none of
/// it; the app validates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunSource {
    /// The argument. Other users of the computer can read a process's
    /// arguments, so a program passes the address on standard input.
    Argument(RunAddress),
    /// `-`: the first line of standard input.
    Stdin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WalkrCommand {
    Status,
    Connect {
        address: RunSource,
        /// Also show the section, as `ui panel walkr` does.
        show: bool,
    },
    Disconnect,
    /// Every waiting session leaves the count, in the app only.
    Clear,
}

/// Any argument of a Walkr command may be a misplaced address, so no error
/// here holds the text of one. Only a subcommand that cannot be an address
/// is named.
pub(super) fn command(args: &mut Args) -> Result<WalkrCommand, CliError> {
    let verb = args.verb().ok_or(CliError::MissingArgument {
        command: "walkr",
        what: "status|connect|disconnect|clear",
    })?;
    let command = match verb.to_str() {
        Some("status") => WalkrCommand::Status,
        Some("connect") => {
            let show = args.flag("--show");
            // Not `required_text`: its error would print the address.
            let address = args
                .required("URL")?
                .into_string()
                .map_err(|_| CliError::InvalidValue("URL"))?;
            WalkrCommand::Connect {
                address: if address == "-" {
                    RunSource::Stdin
                } else {
                    RunSource::Argument(RunAddress::new(address))
                },
                show,
            }
        }
        Some("disconnect") => WalkrCommand::Disconnect,
        Some("clear") => WalkrCommand::Clear,
        Some(word) if is_word(word) => return Err(unknown("walkr", verb)),
        _ => return Err(CliError::UnexpectedUnnamed("walkr")),
    };
    // Before the shared `finish`, which names what is left over.
    args.finish_unnamed()?;
    Ok(command)
}

/// A mistyped subcommand: short, and letters and hyphens only, which no
/// address is.
fn is_word(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 16
        && text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
}
