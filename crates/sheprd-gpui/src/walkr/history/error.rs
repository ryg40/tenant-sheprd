//! Why a saved session could not be shown. No variant holds text of the
//! session: its files are untrusted data.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("The saved Walkr session is not valid JSON or a field has the wrong type")]
    Invalid(#[source] serde_json::Error),
    #[error("The name is not a Walkr session ID")]
    InvalidId,
    #[error("The saved Walkr session folder has no record")]
    Missing,
    #[error("No home folder is set, so no saved Walkr session is found")]
    NoHome,
    #[error("The saved Walkr session is a symlink or not a regular file")]
    NotRegular,
    #[error("Could not open the saved Walkr session")]
    Open(#[source] std::io::Error),
    #[error("Could not read the saved Walkr session")]
    Read(#[source] std::io::Error),
    #[error("The record is not a Walkr Compare session")]
    Schema,
    #[error("The record names another session than its folder")]
    SessionMismatch,
    #[error("The saved Walkr session could not be read in time")]
    TimedOut,
    #[error("The saved Walkr session is larger than 16 MiB")]
    TooLarge,
    #[error("The saved Walkr session has version {0}; Sheprd reads version 1")]
    UnknownVersion(u64),
}

impl Error {
    /// An I/O failure of a read, with the time limit told apart.
    pub(super) fn read(error: std::io::Error) -> Self {
        if error.kind() == std::io::ErrorKind::TimedOut {
            Self::TimedOut
        } else {
            Self::Read(error)
        }
    }
}
