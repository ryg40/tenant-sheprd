//! What `browser` does to a page tab beyond opening it: load another
//! address, go back or forward, zoom it, read its text, and picture it. Only
//! builds that show pages have any; the rest answer that they cannot.
use super::{find, kind};
#[cfg(not(target_os = "macos"))]
use crate::control::server::{Watches, socket};
use crate::{
    browser::{Location, Store, Tab, WebUrl},
    control::protocol::{
        Caller, ErrorCode, Response,
        tabs::{Reply, ZoomChange},
    },
};
use gpui::App;

#[cfg(embedded_pages)]
mod embedded;
#[cfg(target_os = "macos")]
pub(super) use embedded::screenshot;
#[cfg(embedded_pages)]
pub(super) use embedded::{history, text};

/// The most a page's text may take in an answer, as JSON, leaving room
/// for the rest of the answer within the socket's bound.
#[cfg(embedded_pages)]
const MAX_TEXT_JSON: usize = 240 * 1024;

fn no_pages() -> Response {
    Response::error(
        ErrorCode::Unsupported,
        "This platform's build shows pages in the system browser, not in tabs",
    )
}

/// The tab `id`, if it is a page: a web address or a local file.
fn page_tab(id: u64, cx: &App) -> Result<Tab, Response> {
    let tab = find(id, cx)?;
    if !kind(&tab).is_page() {
        return Err(Response::error(
            ErrorCode::InvalidRequest,
            format!("Tab {id} is not a page; `browser list` lists them"),
        ));
    }
    Ok(tab)
}

/// `text` cut at a character boundary until its JSON form fits `limit`.
#[cfg(any(embedded_pages, test))]
fn bounded_text(text: &str, limit: usize) -> String {
    let mut text = text;
    loop {
        let size = serde_json::to_string(text).map_or(usize::MAX, |json| json.len());
        if size <= limit {
            return text.to_owned();
        }
        // Escapes make JSON longer than the text, so cut in proportion.
        let mut end = text.len().saturating_mul(limit) / size.max(1);
        end = end.min(text.len().saturating_sub(1));
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text = &text[..end];
    }
}

/// Loads another web address in a page tab. A window that made the page
/// loads it at once; any other shows it when it next makes the page.
pub(super) fn navigate(caller: &Caller, id: u64, url: &str, cx: &mut App) -> Response {
    if !crate::browser::EMBEDDED {
        return no_pages();
    }
    let location = match WebUrl::try_from(url) {
        Ok(url) => Location::Web { url },
        Err(error) => return Response::error(ErrorCode::InvalidUrl, error.to_string()),
    };
    let tab = match page_tab(id, cx) {
        Ok(tab) => tab,
        Err(response) => return response,
    };
    // A tab is shared by every window, so a named window must be one that
    // shows it: a wrong `--window` never replaces another window's page.
    if let Some(window) = caller.window
        && super::showing(caller, &tab, cx).is_none()
    {
        return super::not_found(format!("Window {window} does not show tab {id}"));
    }
    Store::update(cx, |store| store.asked(tab.id, location.clone()));
    load_everywhere(&tab, &location, cx);
    Response::done(format!("Loading {url} in tab {id}"))
}

/// Zooms a page tab. The zoom is the tab's, so every window's page for it
/// follows, and a window that makes the page later makes it at that zoom.
pub(super) fn zoom(caller: &Caller, id: u64, change: ZoomChange, cx: &mut App) -> Response {
    if !crate::browser::EMBEDDED {
        return no_pages();
    }
    let tab = match page_tab(id, cx) {
        Ok(tab) => tab,
        Err(response) => return response,
    };
    if let Some(window) = caller.window
        && super::showing(caller, &tab, cx).is_none()
    {
        return super::not_found(format!("Window {window} does not show tab {id}"));
    }
    let zoom = match tab.zoom.changed(change) {
        Ok(zoom) => zoom,
        Err(error) => return Response::error(ErrorCode::InvalidRequest, error.to_string()),
    };
    Store::update(cx, |store| store.set_zoom(tab.id, zoom));
    for handle in crate::control::main_windows(cx) {
        let _ = handle.update(cx, |view, _, _| view.apply_page_zoom(tab.id, zoom));
    }
    Response::Tab {
        reply: Reply::Zoom {
            id,
            percent: zoom.percent(),
        },
    }
}

#[cfg(embedded_pages)]
fn load_everywhere(tab: &Tab, location: &Location, cx: &mut App) {
    for handle in crate::control::main_windows(cx) {
        let _ = handle.update(cx, |view, _, _| {
            view.load_requested_page(tab.id, location);
        });
    }
}

#[cfg(not(embedded_pages))]
fn load_everywhere(_: &Tab, _: &Location, _: &mut App) {}

#[cfg(not(embedded_pages))]
pub(super) fn history(_: &Caller, _: u64, _: bool, _: &mut App) -> Response {
    no_pages()
}

#[cfg(not(embedded_pages))]
pub(super) fn text(_: Caller, _: u64, incoming: socket::Incoming, _: &mut Watches, _: &mut App) {
    incoming.respond(no_pages());
}

/// WebKit's snapshot is the one way to picture a page, and only macOS
/// builds call it.
#[cfg(not(target_os = "macos"))]
pub(super) fn screenshot(
    _: Caller,
    _: u64,
    _: String,
    incoming: socket::Incoming,
    _: &mut Watches,
    _: &mut App,
) {
    incoming.respond(Response::error(
        ErrorCode::Unsupported,
        "Screenshots of pages need the macOS build",
    ));
}

/// Opens the question in the caller's window, as every command that asks
/// does; nothing is removed from here. The answer says what the user's
/// confirmation does not remove at once.
#[cfg(servo_pages)]
pub(super) fn clear_data(caller: &Caller, cx: &mut App) -> Response {
    let handle = match crate::control::server::window_for(caller, cx) {
        Ok(handle) => handle,
        Err(refused) => return refused,
    };
    let asked = handle.update(cx, |view, window, cx| {
        let asked = view.ask_to_clear_browser_data(window, cx);
        if asked {
            crate::control::server::work::bring_forward(window, cx);
        }
        asked
    });
    match asked {
        Ok(true) if crate::browser::engine::holds_data() => Response::done(
            "Asked to clear the cookies and site storage of page tabs; nothing is removed until the user confirms. Page tabs were opened in this run: cookies go at once, and the removal of site storage completes when Sheprd is restarted",
        ),
        Ok(true) => Response::done(
            "Asked to clear the cookies and site storage of page tabs; nothing is removed until the user confirms",
        ),
        Ok(false) => super::busy(),
        Err(_) => super::closed(),
    }
}

/// Only Servo's pages keep their data in a folder of the app. WebKit and
/// WebView2 keep theirs in stores of their own, which this command does
/// not reach.
#[cfg(not(servo_pages))]
pub(super) fn clear_data(_: &Caller, _: &mut App) -> Response {
    Response::error(
        ErrorCode::Unsupported,
        "Clearing the data of page tabs needs a Linux build with Servo",
    )
}

#[cfg(test)]
mod tests;
