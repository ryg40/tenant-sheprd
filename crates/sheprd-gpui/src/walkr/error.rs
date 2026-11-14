//! Walkr setup and worker failures. Transport failures are connectivity states.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("The Walkr address is longer than 2,048 bytes")]
    AddressTooLong,
    #[error(transparent)]
    History(#[from] super::history::Error),
    #[error(
        "The Walkr address must be http on 127.0.0.1, localhost or [::1], with a port and no user info"
    )]
    InvalidAddress,
    #[error(
        "walkr.probe_path must start with /, contain no query, fragment, whitespace or control characters, and be at most 256 bytes"
    )]
    InvalidProbePath,
    #[error("walkr.url must be an HTTP or HTTPS address with no user info, query, or fragment")]
    InvalidUrl,
    // Native diagnostics may contain the run address. Keep their typed
    // causes for callers, but never include them in the display text.
    #[cfg(any(target_os = "macos", windows))]
    #[error("Could not create the Walkr page")]
    PageConstruction(#[source] Box<crate::Error>),
    #[cfg(any(target_os = "macos", windows))]
    #[error("Could not navigate the Walkr page")]
    PageNavigation(#[source] wry::Error),
    #[cfg(any(target_os = "macos", windows))]
    #[error("Could not reload the Walkr page")]
    PageReload(#[source] wry::Error),
    #[cfg(windows)]
    #[error("Walkr requires WebView2 Runtime 101.0.1210.39 or later for a private page")]
    PrivateRuntimeTooOld,
    #[cfg(windows)]
    #[error("Could not check the WebView2 Runtime for a private Walkr page")]
    PrivateRuntimeUnavailable(#[source] wry::Error),
    #[error(
        "64 panes hold this Walkr run already. Disconnect one of them, or close it, before another connects"
    )]
    TooManyPanes,
    #[error(transparent)]
    Waiting(#[from] super::waiting::Error),
    /// The probe worker or the saved-session reader could not start.
    #[error("Could not start a background thread for Walkr")]
    Worker(#[source] std::io::Error),
}
