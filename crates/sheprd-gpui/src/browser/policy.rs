//! What a page may do, whichever backend draws it. Every page is untrusted:
//! it leaves only for web addresses, it loads no file of this computer but
//! through the preview scheme, the preview scheme answers only a local page,
//! and only a local page's own frames reach the bridge. Pure rules, so they
//! are tested without a window; `native` (wry) and `engine` (Servo) apply
//! them in their handlers.
use super::WebUrl;
#[cfg(any(servo_pages, test))]
use super::location::PostedFrom;

/// Longer posts are dropped before they are queued.
pub(super) const MAX_POST_BYTES: usize = 64 * 1024;

/// Pages leave only for web addresses and the preview scheme. Anything
/// else, such as `file:` or an application's custom scheme, stays unopened
/// rather than reaching the OS.
pub(super) fn navigable(url: &str) -> bool {
    url == "about:blank"
        || url.starts_with("about:srcdoc")
        || url.starts_with("herdr-preview://localhost/")
        || WebUrl::try_from(url).is_ok()
}

/// The path a Servo page's `window.ipc.postMessage` asks the preview scheme
/// for, with the message as its `m` parameter. Servo has no channel from a
/// page to its host, so the request itself carries the message.
#[cfg(any(servo_pages, test))]
pub(super) const POST_PATH: &str = "/.herdr/post";

/// Whether `url` is in the preview scheme's one origin, as Servo spells it.
/// Not the `http://herdr-preview.localhost` WebView2 writes: on Linux that is
/// a web address, which a server of this computer could answer.
#[cfg(any(servo_pages, test))]
fn in_preview(url: &str) -> bool {
    url == "herdr-preview://localhost" || url.starts_with("herdr-preview://localhost/")
}

/// Whether a Servo page may load `url` as a document or a resource. Servo
/// always carries a `file:` handler, which any page could ask for an image
/// or a script of this computer; so only the web, the preview scheme, and
/// the schemes that hold their own bytes load.
#[cfg(any(servo_pages, test))]
pub(super) fn loadable(url: &url::Url) -> bool {
    match url.scheme() {
        "http" | "https" | "data" | "blob" | "about" => true,
        "herdr-preview" => url.host_str() == Some("localhost"),
        _ => false,
    }
}

/// Whether the preview scheme refuses a Servo page's request: one that a
/// web page made. `referrer` is the address of the document that asked,
/// when Servo names one; `main_frame` is whether the request navigates the
/// tab itself; `top` is the address the tab shows. A request without a
/// referrer is a navigation the app or the user started, or comes from a
/// document that hides its address, which is believed only in a tab that
/// shows a local page.
#[cfg(any(servo_pages, test))]
pub(super) fn preview_refused(referrer: Option<&str>, main_frame: bool, top: Option<&str>) -> bool {
    match referrer {
        Some(referrer) => !in_preview(referrer),
        None => !main_frame && !top.is_some_and(in_preview),
    }
}

/// What happens to one request of a Servo page.
#[cfg(any(servo_pages, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Verdict {
    /// It fails, as a network error.
    Refuse,
    /// Servo's network answers it: the web.
    Network,
    /// It is a message for the app ([`posted`]), and carries no file.
    Post,
    /// The preview scheme answers it: from the page's folder when `folder`,
    /// and with a refusal otherwise.
    Serve { folder: bool },
}

/// Decides one request of a Servo page for `url`, with `referrer`,
/// `main_frame`, and `top` as [`preview_refused`] reads them. The order is
/// the rule: nothing loads that [`loadable`] refuses, the web never reaches
/// the preview scheme, and a message is read before any file is looked for,
/// so its path is never served.
#[cfg(any(servo_pages, test))]
pub(super) fn verdict(
    url: &url::Url,
    referrer: Option<&str>,
    main_frame: bool,
    top: Option<&str>,
) -> Verdict {
    if !loadable(url) {
        return Verdict::Refuse;
    }
    if url.scheme() != super::location::PREVIEW_SCHEME {
        return Verdict::Network;
    }
    if url.path() == POST_PATH {
        return Verdict::Post;
    }
    Verdict::Serve {
        folder: !preview_refused(referrer, main_frame, top),
    }
}

/// The message a Servo page posted with `url`, a request for [`POST_PATH`],
/// and the frame it came from, by the request's referrer. `None` for a
/// message that is too long or missing. A frame Servo does not name is not
/// believed to be a local page's.
#[cfg(any(servo_pages, test))]
pub(super) fn posted(url: &url::Url, referrer: Option<&str>) -> Option<(String, PostedFrom)> {
    let (_, body) = url.query_pairs().find(|(name, _)| name == "m")?;
    if body.len() > MAX_POST_BYTES {
        return None;
    }
    let from = match referrer {
        Some(referrer) if in_preview(referrer) => PostedFrom::Preview,
        _ => PostedFrom::Elsewhere,
    };
    Some((body.into_owned(), from))
}

#[cfg(test)]
mod tests;
