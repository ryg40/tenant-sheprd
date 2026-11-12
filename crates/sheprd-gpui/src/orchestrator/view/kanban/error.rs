//! Failures retained until the board's display boundary.
use crate::i18n::fl;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error("a board operation is busy")]
    Busy,
    #[error(transparent)]
    Encode(#[from] serde_json::Error),
    #[error(transparent)]
    Model(#[from] sheprd_orchestrator::kanban::Error),
    #[error("lane order exceeds its storage limit")]
    OrderLimit,
    #[error("board preparation is pending")]
    Pending,
    #[error("board record exceeds the response limit")]
    TooLarge,
    #[error("lane is unknown or no longer belongs to this board")]
    UnknownLane,
    // keep-sorted end
}

impl Error {
    pub(crate) fn localize(&self) -> String {
        match self {
            Self::Busy => fl!("kanban-order-busy"),
            Self::OrderLimit => fl!("kanban-order-limit"),
            Self::Pending => fl!("kanban-pending"),
            Self::UnknownLane => fl!("kanban-order-unknown"),
            Self::TooLarge => fl!("kanban-too-large"),
            Self::Model(_) | Self::Encode(_) => fl!("kanban-unavailable"),
        }
    }
}
