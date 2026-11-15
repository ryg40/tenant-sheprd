//! Why a selection change was refused; nothing changed then.

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    // keep-sorted start
    #[error("too many repository keys")]
    TooMany,
    #[error("unknown or oversized repository key")]
    Unknown,
    // keep-sorted end
}
