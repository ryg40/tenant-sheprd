//! The `artifacts` commands' wire calls and text output.
use super::{Context, Output, unexpected};
use crate::{
    cli::control::ArtifactsCommand,
    control::protocol::{
        Request, Response,
        artifacts::{ArtifactsOp, Reply, State},
    },
};

pub(super) fn run(command: ArtifactsCommand, context: &Context) -> crate::Result<Output> {
    match command {
        ArtifactsCommand::Status => status(context),
        ArtifactsCommand::Start => context.act(Request::Artifacts, ArtifactsOp::Start {}),
        ArtifactsCommand::Off => context.act(Request::Artifacts, ArtifactsOp::Off {}),
        ArtifactsCommand::Open => context.act(Request::Artifacts, ArtifactsOp::Open {}),
    }
}

fn status(context: &Context) -> crate::Result<Output> {
    let status = match context.call(Request::Artifacts, ArtifactsOp::Status {})? {
        Response::Artifacts {
            reply: Reply::Status(status),
        } => status,
        response => return Err(unexpected(response)),
    };
    let state = match status.state {
        State::Running => "The local artifact server runs",
        State::Stopped => {
            "The local artifact server is stopped; sheprd artifacts start asks to start it"
        }
        State::Remote => "The remote artifact server answers",
        State::Unreachable => "The artifact server does not answer",
        State::Unconfigured => "The artifacts program is not set up; run artifacts setup",
    };
    let page = status
        .url
        .as_deref()
        .map_or_else(String::new, |url| format!("\nPage: {url}"));
    let opens = status.opens.as_deref().map_or_else(String::new, |url| {
        format!("\nThe Artifacts button opens {url}")
    });
    Output::data(format!("{state}{page}{opens}"), &status)
}
