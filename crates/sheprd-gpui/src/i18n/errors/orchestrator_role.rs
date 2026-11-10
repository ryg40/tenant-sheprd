//! Agent role failures from `error-orchestrator-role.ftl`: a config entry
//! the app refuses, or a prompt file the agent's host lacks.

use super::Localize;
use crate::i18n::fl;
use sheprd_orchestrator::role::Error;

impl Localize for Error {
    fn localize(&self) -> String {
        match self {
            // keep-sorted start
            Self::Agent { role, agent } => fl!(
                "error-orchestrator-role-agent",
                role = format!("{role:?}"),
                agent = format!("{agent:?}")
            ),
            Self::Missing { path } => fl!(
                "error-orchestrator-role-missing",
                path = format!("{path:?}")
            ),
            Self::Name { max } => fl!("error-orchestrator-role-name", max = *max),
            Self::PromptFile { role } => fl!(
                "error-orchestrator-role-prompt-file",
                role = format!("{role:?}")
            ),
            Self::TooMany(max) => fl!("error-orchestrator-role-too-many", max = *max),
            // keep-sorted end
        }
    }
}
