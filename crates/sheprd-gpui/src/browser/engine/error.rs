//! Why Servo could not draw a page, or keep or clear what sites store.
use std::sync::Arc;

#[derive(Clone, Debug, thiserror::Error)]
pub enum Error {
    #[error("The page's address is not one Servo can load.")]
    Address,
    #[error("The engine that draws pages is busy; the request was dropped.")]
    Busy,
    #[error(transparent)]
    Profile(#[from] super::profile::Error),
    #[error("The page's script stopped: Servo's thread that ran it ended.")]
    ScriptEnded,
    #[error("The engine that draws pages stopped. Restart Sheprd to open pages again.")]
    Stopped,
    #[error("Servo could not create a drawing surface for the page: {0}")]
    Surface(String),
    #[error("Could not start the thread that draws pages: {0}")]
    Thread(#[source] Arc<std::io::Error>),
}
