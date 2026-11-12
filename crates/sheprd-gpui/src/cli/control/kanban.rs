//! Board reads deliberately accept no repository or account selector.
use super::{Args, CliError, OrchestratorCommand};
use sheprd_control_protocol::protocol::orchestrator::MAX_CURSOR_BYTES;

pub(super) fn parse(
    args: &mut Args,
    repo: bool,
    forgejo: bool,
) -> Result<OrchestratorCommand, CliError> {
    if repo || forgejo {
        return Err(CliError::Unexpected {
            command: "orchestrator board",
            argument: if repo { "--repo" } else { "--forgejo" }.into(),
        });
    }
    let cursor = args.text_option("--cursor")?;
    let before = args.text_option("--before")?;
    if let Some(verb) = args.verb() {
        if verb != "order" || cursor.is_some() {
            return Err(CliError::Unexpected {
                command: "orchestrator board",
                argument: verb,
            });
        }
        let lane = args.required_text("LANE")?;
        if crate::orchestrator::view::kanban::order::parse(&lane).is_none()
            || before
                .as_deref()
                .is_some_and(|key| crate::orchestrator::view::kanban::order::parse(key).is_none())
        {
            return Err(CliError::InvalidValue("LANE"));
        }
        return Ok(OrchestratorCommand::BoardOrder { lane, before });
    }
    if before.is_some() {
        return Err(CliError::InvalidValue("--before"));
    }
    if cursor
        .as_ref()
        .is_some_and(|cursor| cursor.is_empty() || cursor.len() > MAX_CURSOR_BYTES)
    {
        return Err(CliError::InvalidValue("--cursor"));
    }
    Ok(OrchestratorCommand::Board { cursor })
}
