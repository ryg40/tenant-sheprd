//! Why a role cannot be offered or taken: a `[[agent_roles]]` entry the
//! config refuses, or a prompt file the agent's host does not have.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error("agent_roles: {role:?} names {agent:?}, which is not an agent Herdr knows")]
    Agent { role: String, agent: String },
    #[error("The role's prompt file {path:?} is not on the agent's host")]
    Missing { path: String },
    #[error("agent_roles: each role needs a name of 1 to {max} characters")]
    Name { max: usize },
    #[error("agent_roles: the prompt_file of {role:?} must be an absolute path or start with ~/")]
    PromptFile { role: String },
    #[error("agent_roles: at most {0} roles")]
    TooMany(usize),
    // keep-sorted end
}
