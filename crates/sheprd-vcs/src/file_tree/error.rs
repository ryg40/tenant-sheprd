//! File tree failures.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error("Listing the folder was cancelled")]
    Cancelled,
    #[error("This is not a file of the checkout")]
    NotFile,
    #[error("This is not a folder of the checkout")]
    NotFolder,
    #[error("The path is outside the checkout")]
    Outside,
    #[error("The folder could not be read")]
    Unreadable(#[source] std::io::Error),
    // keep-sorted end
}
