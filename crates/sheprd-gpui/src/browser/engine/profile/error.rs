//! Why the folder that keeps what sites store could not be used or cleared.
use std::sync::Arc;

#[derive(Clone, Debug, thiserror::Error)]
pub enum Error {
    #[error("Another Sheprd has the folder that keeps the data of browser tabs open.")]
    InUse,
    #[error("Could not reach the folder that keeps the data of browser tabs.")]
    Io(#[source] Arc<std::io::Error>),
    #[error("The folder that keeps the data of browser tabs is not a folder of this user.")]
    NotOwn,
    #[error(
        "The engine that draws pages did not answer. The data of browser tabs is removed when Sheprd next starts."
    )]
    Unanswered,
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(Arc::new(error))
    }
}

impl From<rustix::io::Errno> for Error {
    fn from(error: rustix::io::Errno) -> Self {
        Self::Io(Arc::new(error.into()))
    }
}
