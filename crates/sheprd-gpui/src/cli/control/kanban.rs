//! Board operations accept canonical identities, never approval flags or source selectors.
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
    let lane = args.text_option("--lane")?;
    let to = args.text_option("--to")?;
    if let Some(verb) = args.verb() {
        if verb == "move" && cursor.is_none() && before.is_none() {
            let item = args.required_text("ITEM")?;
            let parsed = item
                .parse::<sheprd_orchestrator::ItemKey>()
                .map_err(|_| CliError::InvalidValue("ITEM"))?;
            if parsed.canonical() != item {
                return Err(CliError::InvalidValue("ITEM"));
            }
            let lane = lane.ok_or(CliError::InvalidValue("--lane"))?;
            if crate::orchestrator::view::kanban::order::parse(&lane).is_none() {
                return Err(CliError::InvalidValue("LANE"));
            }
            use sheprd_control_protocol::BoardColumn;
            let to = match to.as_deref() {
                Some("open") => BoardColumn::Open,
                Some("in-progress") => BoardColumn::InProgress,
                Some("in-review") => BoardColumn::InReview,
                Some("done") => BoardColumn::Done,
                _ => return Err(CliError::InvalidValue("--to")),
            };
            return Ok(OrchestratorCommand::BoardMove { item, lane, to });
        }
        if verb != "order" || cursor.is_some() || lane.is_some() || to.is_some() {
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
    if before.is_some() || lane.is_some() || to.is_some() {
        return Err(CliError::InvalidValue("orchestrator board"));
    }
    if cursor
        .as_ref()
        .is_some_and(|cursor| cursor.is_empty() || cursor.len() > MAX_CURSOR_BYTES)
    {
        return Err(CliError::InvalidValue("--cursor"));
    }
    Ok(OrchestratorCommand::Board { cursor })
}
