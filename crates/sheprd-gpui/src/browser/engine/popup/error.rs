//! Why the engine could not make the page of a page's new window.

/// Servo's own error for a drawing surface has only a `Debug` form, and
/// Servo does not export its name, so no variant keeps it.
#[derive(Debug, thiserror::Error)]
pub(in crate::browser) enum Error {
    #[error("Servo could not create a drawing surface for the page of a page's new window.")]
    Surface,
}
