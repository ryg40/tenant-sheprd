//! `orchestrator repos [list]` and `orchestrator repos set KEY... | --all`.
use super::{Args, CliError, OrchestratorCommand};
use crate::orchestrator::view::repo_select;
use sheprd_control_protocol::protocol::orchestrator::repo_keys_bounded;

pub(super) fn parse(
    args: &mut Args,
    repo: bool,
    forgejo: bool,
) -> Result<OrchestratorCommand, CliError> {
    if repo || forgejo {
        return Err(CliError::Unexpected {
            command: "orchestrator repos",
            argument: if repo { "--repo" } else { "--forgejo" }.into(),
        });
    }
    let all = args.flag("--all");
    let verb = args.verb();
    match verb.as_ref().and_then(|verb| verb.to_str()) {
        None | Some("list") if !all => Ok(OrchestratorCommand::Repos),
        Some("set") => {
            let mut keys = Vec::new();
            while let Some(key) = args.optional_text()? {
                keys.push(key);
            }
            if all {
                if !keys.is_empty() {
                    return Err(CliError::Unexpected {
                        command: "orchestrator repos set",
                        argument: "--all".into(),
                    });
                }
                return Ok(OrchestratorCommand::ReposSet { keys: None });
            }
            if keys.is_empty() {
                return Err(CliError::MissingArgument {
                    command: "orchestrator repos set",
                    what: "KEY... or --all",
                });
            }
            if !repo_keys_bounded(&keys) || keys.iter().any(|key| repo_select::parse(key).is_none())
            {
                return Err(CliError::InvalidValue("KEY"));
            }
            Ok(OrchestratorCommand::ReposSet { keys: Some(keys) })
        }
        _ => Err(CliError::Unexpected {
            command: "orchestrator repos",
            argument: verb.unwrap_or_else(|| "--all".into()),
        }),
    }
}
