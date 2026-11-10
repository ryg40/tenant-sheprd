//! What a page tells its window, whichever backend draws it: wry's native
//! view or Servo. Handlers run on the platform's callbacks or on Servo's
//! thread, so they only queue these for the window to apply on its next
//! tick, in a bounded queue.
use super::TabId;
#[cfg(target_os = "macos")]
use super::WebUrl;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::{SyncSender, TrySendError},
};

/// Enough for a burst of title and load reports between two window ticks;
/// later ones are dropped rather than queued without bound.
pub(super) const EVENT_CAPACITY: usize = 64;

/// What a page reported, from the tab `P` names. Handlers run on the
/// platform's callbacks, so they only queue these for the window to apply on
/// its next tick.
#[derive(Debug)]
pub(crate) enum Event<P = TabId> {
    Title(P, String),
    Loaded(P, String),
    /// The page asked for a new window, which becomes a new, unrelated tab.
    /// macOS asks for this only when it cannot build the popup itself.
    NewWindow(P, String),
    /// The page opened a popup, already built and linked to it, which the
    /// window adopts as a new tab. Only macOS builds them.
    #[cfg(target_os = "macos")]
    Opened(P, WebUrl, Source, super::popup::Popup),
    /// A popup's script closed it, which closes its tab.
    #[cfg(target_os = "macos")]
    Closed(P),
    /// A screenshot for a note, as TIFF bytes, or `None` when WebKit had
    /// none. Only macOS takes them.
    #[cfg(target_os = "macos")]
    Captured(P, u64, Option<Vec<u8>>),
    /// A message the annotation picker or the page bridge posted, and the
    /// frame that posted it. Any script in any frame can post one, so it is
    /// parsed as untrusted input.
    Posted(P, String, super::PostedFrom),
    /// Servo could not make the page.
    #[cfg(servo_pages)]
    Failed(P, super::engine::Error),
    /// The page finished loading and has no title of its own. Only Servo
    /// says so; an empty title from the other backends is ignored.
    #[cfg(servo_pages)]
    Untitled(P),
}

/// The page a report came from. A popup's page exists before its tab does,
/// so its reports name the popup until the window adopts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Source {
    Tab(TabId),
    #[cfg(target_os = "macos")]
    Popup(u64),
}

impl<P> Event<P> {
    /// The report's kind and the page it came from, without what the page
    /// put in it: a title, an address, and a posted message are the page's
    /// text, and none of it belongs in a log.
    fn origin(&self) -> (&'static str, &P) {
        match self {
            Self::Title(source, _) => ("title", source),
            Self::Loaded(source, _) => ("loaded", source),
            Self::NewWindow(source, _) => ("new-window", source),
            #[cfg(target_os = "macos")]
            Self::Opened(source, ..) => ("opened", source),
            #[cfg(target_os = "macos")]
            Self::Closed(source) => ("closed", source),
            #[cfg(target_os = "macos")]
            Self::Captured(source, ..) => ("captured", source),
            Self::Posted(source, ..) => ("posted", source),
            #[cfg(servo_pages)]
            Self::Failed(source, _) => ("failed", source),
            #[cfg(servo_pages)]
            Self::Untitled(source) => ("untitled", source),
        }
    }
}

impl Event<Source> {
    /// The same report from the tab `tab` finds for its source, or `None`
    /// for a popup no tab adopted.
    pub(super) fn resolve(self, tab: impl Fn(Source) -> Option<TabId>) -> Option<Event> {
        Some(match self {
            Self::Title(source, title) => Event::Title(tab(source)?, title),
            Self::Loaded(source, url) => Event::Loaded(tab(source)?, url),
            Self::NewWindow(source, url) => Event::NewWindow(tab(source)?, url),
            #[cfg(target_os = "macos")]
            Self::Opened(source, url, popup, page) => Event::Opened(tab(source)?, url, popup, page),
            #[cfg(target_os = "macos")]
            Self::Closed(source) => Event::Closed(tab(source)?),
            #[cfg(target_os = "macos")]
            Self::Captured(source, capture, tiff) => Event::Captured(tab(source)?, capture, tiff),
            Self::Posted(source, body, from) => Event::Posted(tab(source)?, body, from),
            #[cfg(servo_pages)]
            Self::Failed(source, error) => Event::Failed(tab(source)?, error),
            #[cfg(servo_pages)]
            Self::Untitled(source) => Event::Untitled(tab(source)?),
        })
    }
}

/// Reports dropped since the app started.
static DROPPED: AtomicU64 = AtomicU64::new(0);

/// Whether the drop that makes the count `dropped` gets a log line: the
/// first, and then each that doubles the count. A page that keeps the queue
/// full writes a few dozen lines however long it runs, and each line gives
/// the count.
fn summarized(dropped: u64) -> bool {
    dropped.is_power_of_two()
}

pub(super) fn report(sender: &SyncSender<Event<Source>>, event: Event<Source>) {
    if let Err(TrySendError::Full(event)) = sender.try_send(event) {
        let dropped = DROPPED.fetch_add(1, Ordering::Relaxed).saturating_add(1);
        if summarized(dropped) {
            let (kind, source) = event.origin();
            tracing::debug!(kind, ?source, dropped, "Dropped a browser event");
        }
    }
}

#[cfg(test)]
mod tests;
