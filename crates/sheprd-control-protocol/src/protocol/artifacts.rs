//! The local artifact server as the `artifacts` program of the user reports
//! it. The app runs that program; this crate only names its states and the
//! requests of `sheprd artifacts`.
use serde::{Deserialize, Serialize};

/// The longest a request waits for the program's status answer: its own
/// health check takes at most 5 seconds, and Python starts first.
pub const WAIT_SECONDS: u64 = 20;

/// What `artifacts --json status` calls the server. The app parses the
/// program's answer into this enum, so an unknown state is a parse error.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// A local server of this user answers.
    Running,
    /// A local server is configured and none runs.
    Stopped,
    /// The configured origin is a remote server, and it answers.
    Remote,
    /// The configured server does not answer.
    Unreachable,
    /// The program has no configuration.
    Unconfigured,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArtifactsOp {
    // Empty struct variants make serde reject extra operation fields.
    /// The state and the page address, from the program.
    Status {},
    /// Opens the app's question to start the local server; never starts it.
    Start {},
    /// Opens the app's question to stop the local server; never stops it.
    Off {},
    /// Opens the page when the server answers; starts nothing.
    Open {},
}

/// The program's answer beside the page of the server it describes, and
/// the Artifacts button's page when that is another one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub state: State,
    /// The page of the server `state` describes: the program's browse
    /// page, or `[artifacts] url` when the program names none. `None` only
    /// when neither names one.
    pub url: Option<String>,
    /// The page the button opens when it is not `url`: `[artifacts] url`
    /// set to another page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opens: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reply {
    Status(Status),
}

#[cfg(test)]
mod tests;
