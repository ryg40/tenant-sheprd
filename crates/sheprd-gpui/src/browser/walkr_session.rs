//! The Walkr section's page of a saved session: the document
//! `walkr::document` makes from the bounded record, served from memory by
//! the preview scheme. It is a page of its own beside the section's page of
//! a run, so a pick never closes a run's page, live or kept. The text view
//! shows while the document is made and loads, and stays when the page
//! fails; see `docs/walkr.md`.
use super::{TabId, document::Document, kit, page::PageElement};
use crate::{SheprdWindow, sidebar::ActivityPanel, walkr::sessions::Shown};
use gpui::{Context, Window};

/// What a document shows: one session, with as many ratings, in one theme.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Key {
    id: String,
    ratings: usize,
    theme: kit::ThemeKey,
}

#[derive(Debug, Default)]
pub(crate) struct SessionPage {
    /// What the current document shows.
    built: Option<Key>,
    /// What the document being made will show.
    building: Option<Key>,
    /// What the page serves; shared with its scheme handler.
    document: Document,
    /// The session whose page failed: it shows as text until it is picked
    /// again.
    failed: Option<String>,
    /// The page loaded a document of `built`'s session.
    loaded: bool,
    /// Numbers the documents, so each loads at a path of its own.
    serial: u64,
}

impl SheprdWindow {
    /// The document the section would show now: a picked session that is
    /// not live and was read.
    fn walkr_session_wanted(&self) -> Option<Key> {
        if !self.walkr.detail_shown() {
            return None;
        }
        let Some(Shown::Detail(detail)) = self.walkr.sessions.detail() else {
            return None;
        };
        Some(Key {
            id: detail.id.clone(),
            ratings: detail.ratings.len(),
            theme: kit::theme_key(&self.theme),
        })
    }

    /// Whether the page shows the picked session, in place of its text.
    pub(crate) fn shows_walkr_session_page(&self) -> bool {
        let page = &self.walkr.session_page;
        page.loaded
            && page.failed.is_none()
            && page.built.as_ref().map(|built| built.id.as_str()) == self.walkr.sessions.pick()
            && self.walkr_session_wanted().is_some()
            && self.browser.pages.contains(TabId::WALKR_SESSION)
    }

    pub(crate) fn walkr_session_element(&self) -> Option<PageElement> {
        self.shows_walkr_session_page()
            .then(|| self.browser.pages.element(TabId::WALKR_SESSION))
            .flatten()
    }

    /// A tick: makes the document of the shown session and its page, and
    /// closes the page once no session shows. Pages are made only while the
    /// section shows; one already made stays while the section hides.
    pub(crate) fn poll_walkr_session_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Windows reports a build that failed here; Servo, a page that failed.
        if self.browser.failed.remove(&TabId::WALKR_SESSION).is_some() {
            self.fail_walkr_session_page();
            cx.notify();
        }
        let Some(key) = self.walkr_session_wanted() else {
            let page = &self.walkr.session_page;
            if page.built.is_some()
                || page.building.is_some()
                || page.failed.is_some()
                || self
                    .browser
                    .pages
                    .ids()
                    .any(|id| id == TabId::WALKR_SESSION)
            {
                self.close_walkr_session_page();
                cx.notify();
            }
            return;
        };
        let page = &self.walkr.session_page;
        if page.failed.as_deref() == Some(key.id.as_str())
            || page.built.as_ref() == Some(&key)
            || page.building.as_ref() == Some(&key)
        {
            return;
        }
        if self.activity_section() != Some(ActivityPanel::Walkr) || self.mod_app().is_some() {
            return;
        }
        let Some(Shown::Detail(detail)) = self.walkr.sessions.detail() else {
            return;
        };
        // The record is shared: the UI thread copies only its handle.
        let detail = std::sync::Arc::clone(detail);
        // More ratings of the same session open where the added ones start.
        let fragment = page
            .built
            .as_ref()
            .filter(|built| built.id == key.id && built.ratings < key.ratings)
            .map(|built| crate::walkr::document::rating_anchor(built.ratings));
        let theme = kit::theme_css(&self.theme);
        self.walkr.session_page.building = Some(key.clone());
        // A large record takes a moment to escape and copy: off the UI
        // thread, up to the handle the page serves.
        cx.spawn_in(window, async move |this, cx| {
            let html = cx
                .background_executor()
                .spawn(async move {
                    std::sync::Arc::<str>::from(crate::walkr::document::document(&detail, &theme))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.walkr_session_built(key, fragment, html, window, cx);
            });
        })
        .detach();
    }

    /// The document of `key` is made: the page loads it, or a page is made
    /// for it. A document no longer wanted is dropped.
    fn walkr_session_built(
        &mut self,
        key: Key,
        fragment: Option<String>,
        html: std::sync::Arc<str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let page = &mut self.walkr.session_page;
        if page.building.as_ref() != Some(&key) {
            return;
        }
        page.building = None;
        if self.walkr_session_wanted().as_ref() != Some(&key) {
            return;
        }
        let page = &mut self.walkr.session_page;
        page.serial = page.serial.wrapping_add(1);
        let url = page
            .document
            .replace(page.serial, html, fragment.as_deref());
        let same = page.built.as_ref().is_some_and(|built| built.id == key.id);
        page.built = Some(key);
        let document = page.document.clone();
        // The page of the same session keeps showing until the new one
        // paints; one of another session went before this was made.
        if same
            && self
                .browser
                .pages
                .load_document(TabId::WALKR_SESSION, url.clone())
        {
            cx.notify();
            return;
        }
        self.browser.pages.close(TabId::WALKR_SESSION);
        self.walkr.session_page.loaded = false;
        let made =
            self.browser
                .pages
                .ensure_document(TabId::WALKR_SESSION, url, &document, window, cx);
        if let Err(error) = made {
            tracing::debug!(%error, "Cannot make the page of a Walkr session");
            self.fail_walkr_session_page();
        }
        cx.notify();
    }

    /// The page reported a load of `url`.
    pub(crate) fn walkr_session_loaded(&mut self, url: &str, cx: &mut Context<Self>) {
        let page = &mut self.walkr.session_page;
        if !page.loaded && page.document.is_at(url) {
            page.loaded = true;
            cx.notify();
        }
    }

    /// The page failed: the session it showed stays as text.
    fn fail_walkr_session_page(&mut self) {
        self.browser.pages.close(TabId::WALKR_SESSION);
        let page = &mut self.walkr.session_page;
        page.failed = page.built.take().map(|built| built.id);
        page.building = None;
        page.loaded = false;
    }

    #[cfg(test)]
    pub(crate) fn walkr_session_address(&self) -> Option<String> {
        self.walkr.session_page.document.address()
    }

    fn close_walkr_session_page(&mut self) {
        self.browser.pages.close(TabId::WALKR_SESSION);
        self.browser.failed.remove(&TabId::WALKR_SESSION);
        let page = &mut self.walkr.session_page;
        *page = SessionPage {
            serial: page.serial,
            ..SessionPage::default()
        };
    }
}
