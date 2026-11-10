//! Roles a dispatched agent can start with. A `[[agent_roles]]` entry of the
//! app's config names a prompt file on the agent's host, which the agent's
//! own option ([`AgentKind::system_prompt_flag`]) adds to its system prompt.
//! Sheprd ships no role.

mod error;

pub use error::Error;

use serde::Deserialize;
use sheprd_client::shell_quote;
use sheprd_remote::teleport::{AgentKind, Host};
use std::{path::PathBuf, sync::atomic::AtomicBool};

/// The most roles the config may list.
pub const MAX_ROLES: usize = 32;
/// The longest role name, in characters.
const MAX_NAME: usize = 80;
/// The longest prompt file path, in bytes.
const MAX_PATH: usize = 4096;
const CHECKING: &str = "Checking the role's prompt file";

/// One `[[agent_roles]]` entry.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Role {
    /// The label in the dispatch dialog.
    pub name: String,
    /// The agent kinds, by the names Herdr reports, that can take the role.
    pub agents: Vec<String>,
    /// A path on the agent's host; a leading `~/` is that host's home.
    pub prompt_file: String,
}

impl Role {
    /// Whether `kind` can take this role: the role lists it, and the agent
    /// has an option for a system prompt file.
    pub fn fits(&self, kind: AgentKind) -> bool {
        kind.system_prompt_flag().is_some() && self.agents.iter().any(|name| name == kind.name())
    }

    fn check(&self) -> Result<(), Error> {
        let chars = self.name.chars().count();
        if self.name.trim().is_empty()
            || chars > MAX_NAME
            || self.name.chars().any(char::is_control)
        {
            return Err(Error::Name { max: MAX_NAME });
        }
        if let Some(agent) = self
            .agents
            .iter()
            .find(|agent| AgentKind::parse(agent).is_none())
        {
            return Err(Error::Agent {
                role: self.name.clone(),
                agent: agent.clone(),
            });
        }
        let file = &self.prompt_file;
        // An absolute path or `~/` never reads as an option.
        let rooted = file.starts_with('/')
            || file.starts_with("~/")
            || std::path::Path::new(file).is_absolute();
        if !rooted || file.len() > MAX_PATH || file.chars().any(char::is_control) {
            return Err(Error::PromptFile {
                role: self.name.clone(),
            });
        }
        Ok(())
    }
}

/// Checks the config's roles, as the config checks its other values.
pub fn check(roles: &[Role]) -> Result<(), Error> {
    if roles.len() > MAX_ROLES {
        return Err(Error::TooMany(MAX_ROLES));
    }
    roles.iter().try_for_each(Role::check)
}

/// The path of `file` on `host`, with a leading `~/` made that host's home,
/// once the host has the file. Blocks, so callers run it on a worker.
pub(crate) fn prompt_path(
    host: &Host,
    file: &str,
    cancelled: &AtomicBool,
) -> crate::Result<String> {
    let missing = || Error::Missing {
        path: file.to_owned(),
    };
    if host.is_local() {
        let path = match file.strip_prefix("~/") {
            Some(rest) => sheprd_client::local_path::home_dir()
                .ok_or_else(missing)?
                .join(rest),
            None => PathBuf::from(file),
        };
        if !path.is_file() {
            return Err(missing().into());
        }
        return path
            .into_os_string()
            .into_string()
            .map_err(|_| missing().into());
    }
    let body = format!(
        "file={file}\n\
         case $file in \"~/\"*) file=\"$HOME/${{file#??}}\" ;; esac\n\
         if [ -f \"$file\" ]; then printf '%s' \"$file\"; fi\n",
        file = shell_quote(file),
    );
    let output = host
        .capture(&body, cancelled)
        .map_err(crate::error::script(CHECKING))?;
    let path = String::from_utf8(output).map_err(|_| missing())?;
    if path.is_empty() || path.chars().any(char::is_control) {
        return Err(missing().into());
    }
    Ok(path)
}
