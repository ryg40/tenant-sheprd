//! Pure tracker-backed board data. No I/O, local ticket state, or write approval.

pub mod config;
mod error;
pub mod facts;
pub mod history;
pub mod identity;
pub mod moves;
pub mod projection;

pub use error::Error;
pub use sheprd_control_protocol::BoardColumn;

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
pub(crate) mod tests;
