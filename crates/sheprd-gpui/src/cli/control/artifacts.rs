//! `artifacts`: the local artifact server through the user's `artifacts`
//! program. `start` and `off` only ask in the app; the user confirms there.
use super::{Args, CliError, unknown};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactsCommand {
    /// The state and the page, from `artifacts --json status`.
    Status,
    /// Ask in the app to start the stopped local server.
    Start,
    /// Ask in the app to stop the local server.
    Off,
    /// Open the page when the server answers; starts nothing.
    Open,
}

pub(super) fn command(args: &mut Args) -> Result<ArtifactsCommand, CliError> {
    let Some(verb) = args.verb() else {
        return Ok(ArtifactsCommand::Status);
    };
    Ok(match verb.to_str() {
        Some("status") => ArtifactsCommand::Status,
        Some("start") => ArtifactsCommand::Start,
        Some("off" | "stop") => ArtifactsCommand::Off,
        Some("open") => ArtifactsCommand::Open,
        _ => return Err(unknown("artifacts", verb)),
    })
}
