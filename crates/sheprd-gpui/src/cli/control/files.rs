//! `files`: the file explorer on the window's right.
use super::{Args, CliError, Switch, switch, unknown};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilesCommand {
    /// Show, hide, or toggle the explorer; none only reports it.
    Panel { switch: Option<Switch> },
    /// Show the explorer with a file selected.
    Reveal { path: PathBuf },
}

pub(super) fn command(args: &mut Args) -> Result<FilesCommand, CliError> {
    let Some(verb) = args.verb() else {
        return Ok(FilesCommand::Panel { switch: None });
    };
    Ok(match verb.to_str() {
        Some("status") => FilesCommand::Panel { switch: None },
        Some("show" | "hide" | "toggle") => FilesCommand::Panel {
            switch: Some(switch(Some(verb))?),
        },
        Some("reveal") => FilesCommand::Reveal {
            path: args.required("PATH")?.into(),
        },
        _ => return Err(unknown("files", verb)),
    })
}
