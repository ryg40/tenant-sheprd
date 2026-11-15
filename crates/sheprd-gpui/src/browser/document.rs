//! A document the app makes itself, served from memory through the preview
//! scheme: the Walkr section's page of a saved session. The page has no
//! folder, so the scheme answers it this one document and nothing else.
//! The page reads a shared slot, so a newer document loads in the same
//! page.
use super::preview::Answer;
use std::sync::{Arc, PoisonError, RwLock};

/// `url`, an address `Document::replace` returned, as WebView2 loads it.
/// wry maps the private scheme to WebView2's `http` spelling only for the
/// address a page is built with; a later load gets the address as it is,
/// and WebView2 has no such scheme.
#[cfg(windows)]
pub(crate) fn webview2_url(url: &str) -> String {
    match url.strip_prefix(ORIGINS[0]) {
        Some(rest) => format!("{}{rest}", ORIGINS[1]),
        None => url.to_owned(),
    }
}

/// The origin of each spelling of the preview scheme; a document path follows.
const ORIGINS: [&str; 3] = [
    "herdr-preview://localhost",
    "http://herdr-preview.localhost",
    "https://herdr-preview.localhost",
];

struct Current {
    path: String,
    html: Arc<str>,
}

/// What a page of the app's own making shows now. Clones share it.
#[derive(Clone, Default)]
pub(crate) struct Document(Arc<RwLock<Option<Current>>>);

/// The path only: the document is a record's text.
impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let current = self.0.read().unwrap_or_else(PoisonError::into_inner);
        f.debug_tuple("Document") // i18n: data
            .field(&current.as_ref().map(|current| current.path.as_str()))
            .finish()
    }
}

impl Document {
    /// Shows `html` as document `serial` from now on, and returns its
    /// address, with `fragment` when there is one. A serial gives each
    /// document its own path, so a load of it is never a scroll of the last
    /// one, and an old page's report is told apart.
    /// `html` is made off the UI thread, so a large record's copy is too.
    pub(crate) fn replace(&self, serial: u64, html: Arc<str>, fragment: Option<&str>) -> String {
        // i18n: data
        let path = format!("/.sheprd/walkr-session-{serial}.html");
        let url = match fragment {
            Some(fragment) => format!("{}{path}#{fragment}", ORIGINS[0]),
            None => format!("{}{path}", ORIGINS[0]),
        };
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = Some(Current { path, html });
        url
    }

    /// The address of the current document, as `replace` returned it
    /// without a fragment.
    #[cfg(test)]
    pub(crate) fn address(&self) -> Option<String> {
        let current = self.0.read().unwrap_or_else(PoisonError::into_inner);
        current
            .as_ref()
            .map(|current| format!("{}{}", ORIGINS[0], current.path))
    }

    /// Whether the page that reported `loaded` shows the current document.
    pub(crate) fn is_at(&self, loaded: &str) -> bool {
        let loaded = loaded.split('#').next().unwrap_or(loaded);
        let current = self.0.read().unwrap_or_else(PoisonError::into_inner);
        current.as_ref().is_some_and(|current| {
            ORIGINS
                .iter()
                .any(|origin| loaded.strip_prefix(origin) == Some(current.path.as_str()))
        })
    }

    /// The answer to the page's request for `path`: the current document at
    /// its own path, and nothing at any other. `refused` is a request a web
    /// page made, which gets nothing.
    pub(crate) fn answer(&self, path: &str, refused: bool) -> Answer {
        let current = self.0.read().unwrap_or_else(PoisonError::into_inner);
        match current.as_ref() {
            Some(current) if !refused && current.path == path => Answer {
                status: 200,
                content_type: "text/html; charset=utf-8",
                body: current.html.as_bytes().to_vec(),
            },
            _ => Answer {
                status: if refused { 403 } else { 404 },
                content_type: "text/plain",
                body: Vec::new(),
            },
        }
    }
}

#[cfg(test)]
mod tests;
