//! One window's pages on Linux, drawn by Servo: one per browser tab it has
//! shown. The pages themselves live on the engine's thread
//! ([`super::engine`]); this is the window's list of them, with the same
//! requests `native::Pages` answers for wry's views.
use super::{
    Location, Tab, TabId, Zoom,
    engine::{Command, Engine, Latest, Mailbox, Notifier, Open, PageKey},
    page::{Colors, Drawn, Page, PageElement, Retired},
    reports::{EVENT_CAPACITY, Event, Source},
};
use crate::SheprdWindow;
use gpui::{Context, Task, Window};
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::{
        Arc,
        mpsc::{self, Receiver, SyncSender},
    },
};

pub(crate) struct Pages {
    /// The theme's colors the pages' own lists are drawn in.
    colors: Colors,
    pages: HashMap<TabId, Rc<Page>>,
    /// The tab of each page that Servo built for a new window, whose
    /// reports name the popup ([`Source::Popup`]).
    adopted: HashMap<PageKey, TabId>,
    /// Tabs whose page stopped, which offer to open it again until the
    /// window makes their page again ([`super::stopped`]).
    stopped: HashSet<TabId>,
    /// Only reports from this instance may change the Walkr section.
    walkr: Option<PageKey>,
    /// Only reports from this instance may change the page of a saved
    /// Walkr session.
    walkr_session: Option<PageKey>,
    shown: Vec<TabId>,
    /// Where each page drew in the frame being drawn.
    drawn: Drawn,
    reports: SyncSender<Event<Source>>,
    events: Receiver<Event<Source>>,
    /// Woken when a page of this window has a new frame.
    notifier: Arc<Notifier>,
    /// Draws the window again on each such wake; started with the first
    /// page, and ended with the window.
    redraw: Option<Task<()>>,
    /// Frames no page shows any more, whose textures wait to be freed.
    retired: Retired,
    /// The theme the kit serves local pages, and the colors it came from.
    theme: (super::kit::ThemeKey, String),
    /// Whether that theme is light, which every page reports as its color
    /// scheme; `None` until the window sets a theme.
    light: Option<bool>,
}

impl Default for Pages {
    fn default() -> Self {
        let (reports, events) = mpsc::sync_channel(EVENT_CAPACITY);
        Self {
            colors: Colors::default(),
            pages: HashMap::new(),
            adopted: HashMap::new(),
            stopped: HashSet::new(),
            walkr: None,
            walkr_session: None,
            shown: Vec::new(),
            drawn: Drawn::default(),
            reports,
            events,
            notifier: Arc::default(),
            redraw: None,
            retired: Retired::default(),
            theme: Default::default(),
            light: None,
        }
    }
}

impl Pages {
    /// The tabs this window has a page for.
    pub(crate) fn ids(&self) -> impl Iterator<Item = TabId> + '_ {
        self.pages.keys().copied()
    }

    pub(crate) fn contains(&self, id: TabId) -> bool {
        self.pages.contains_key(&id)
    }

    /// Whether the window shows tab `id`'s page now.
    pub(crate) fn shows(&self, id: TabId) -> bool {
        self.shown.contains(&id)
    }

    /// The element tab `id`'s page lays out in, if the window has the page.
    pub(crate) fn element(&self, id: TabId) -> Option<PageElement> {
        let page = self.pages.get(&id)?;
        Some(PageElement::new(id, page.clone(), self.drawn.clone()).themed(self.colors))
    }

    /// Where each page drew in the frame being drawn, filled as they lay out.
    pub(crate) fn drawn(&self) -> Drawn {
        self.drawn.clone()
    }

    /// Starts the next frame's record of where pages draw.
    pub(crate) fn next_frame(&self) {
        self.drawn.borrow_mut().clear();
    }

    /// Creates the page for `tab` if this window has not shown it yet and it
    /// has an address. The first page starts the engine's thread.
    pub(crate) fn ensure(
        &mut self,
        tab: &Tab,
        _: &mut Window,
        cx: &mut Context<SheprdWindow>,
    ) -> crate::Result<()> {
        // A review tab is drawn by the app; only pages get a web view.
        let Some(location) = tab.location.as_ref().filter(|location| location.is_page()) else {
            return Ok(());
        };
        // The folder this page may read. A web page gets none, and a page
        // keeps the folder it was created with wherever it navigates.
        let root = match location {
            Location::Local { file } => Some(file.root().to_owned()),
            Location::Web { .. }
            | Location::Review { .. }
            | Location::Code { .. }
            | Location::Orchestrator { .. }
            | Location::Devices
            | Location::CoreTerminal { .. } => None,
        };
        self.make(tab.id, location.page_url(), root, None, cx)
    }

    /// Creates the Walkr section's page at `address` if the window has none.
    /// No tab stands for it: a tab's `Debug` would hold the whole address.
    pub(crate) fn ensure_walkr(
        &mut self,
        address: &crate::walkr::run::Address,
        _: &mut Window,
        cx: &mut Context<SheprdWindow>,
    ) -> crate::Result<()> {
        self.make(TabId::WALKR, address.expose().to_owned(), None, None, cx)
    }

    /// Creates page `id` at `url`, showing the app's own `document`, if the
    /// window has none. It has no tab and no folder.
    pub(crate) fn ensure_document(
        &mut self,
        id: TabId,
        url: String,
        document: &super::document::Document,
        _: &mut Window,
        cx: &mut Context<SheprdWindow>,
    ) -> crate::Result<()> {
        self.make(id, url, None, Some(document.clone()), cx)
    }

    /// Loads `url` in page `id`; whether the window has the page.
    pub(crate) fn load_document(&mut self, id: TabId, url: String) -> bool {
        let Some(page) = self.pages.get(&id) else {
            return false;
        };
        page.send(|key| Command::Load(key, url))
    }

    /// Creates tab `id`'s page at `url`, unless the window has it.
    fn make(
        &mut self,
        id: TabId,
        url: String,
        root: Option<std::path::PathBuf>,
        document: Option<super::document::Document>,
        cx: &mut Context<SheprdWindow>,
    ) -> crate::Result<()> {
        if self.pages.contains_key(&id) {
            return Ok(());
        }
        let engine = Engine::get()?;
        if self.redraw.is_none() {
            let notifier = self.notifier.clone();
            self.redraw = Some(cx.spawn(async move |this, cx| {
                loop {
                    notifier.notified().await;
                    // The clipboard here, not as the window draws: a read
                    // waits for its answer, also while the window draws
                    // nothing.
                    let served = this.update(cx, |this, cx| {
                        this.browser.pages.serve_clipboard(this.active, cx);
                        cx.notify();
                    });
                    if served.is_err() {
                        break;
                    }
                }
            }));
            engine.send(Command::Theme(self.theme.1.clone()))?;
        }
        let key = super::engine::page_key();
        let source = if id == TabId::WALKR {
            Source::Walkr(key)
        } else if id == TabId::WALKR_SESSION {
            Source::WalkrSession(key)
        } else {
            Source::Tab(id)
        };
        let (mailbox, latest) = (Arc::new(Mailbox::default()), Arc::new(Latest::default()));
        // An engine that did not take the page leaves the tab without one,
        // so showing the tab again asks again.
        engine.send(Command::Open(Box::new(Open {
            key,
            url,
            // Only a local page, which the agent wrote, gets `herdr.send`.
            bridge: root.is_some(),
            root,
            document,
            light: self.light,
            log_console: id != TabId::WALKR,
            source,
            reports: self.reports.clone(),
            mailbox: mailbox.clone(),
            notifier: self.notifier.clone(),
            latest: latest.clone(),
        })))?;
        let page = Page::new(
            key,
            engine,
            mailbox,
            latest,
            (self.reports.clone(), source),
            cx.focus_handle(),
            self.retired.clone(),
        );
        self.pages.insert(id, Rc::new(page));
        self.stopped.remove(&id);
        if id == TabId::WALKR {
            self.walkr = Some(key);
        }
        if id == TabId::WALKR_SESSION {
            self.walkr_session = Some(key);
        }
        Ok(())
    }

    /// Makes tab `id` the window's handle to `popup`, the page that Servo
    /// built for a page's new window, and tells the engine that the tab
    /// holds it. Without that word the engine closes the page, and the
    /// error tells the caller to close the tab, which has no page.
    pub(crate) fn adopt(
        &mut self,
        id: TabId,
        source: Source,
        popup: super::engine::Popup,
        cx: &mut Context<SheprdWindow>,
    ) -> crate::Result<()> {
        let engine = Engine::get()?;
        let (key, mailbox, latest) = popup.adopt()?;
        let page = Page::new(
            key,
            engine,
            mailbox,
            latest,
            (self.reports.clone(), source),
            cx.focus_handle(),
            self.retired.clone(),
        );
        if let Some(light) = self.light {
            page.set_light(light);
        }
        self.pages.insert(id, Rc::new(page));
        self.stopped.remove(&id);
        if let Source::Popup(key) = source {
            self.adopted.insert(key, id);
        }
        Ok(())
    }

    /// Serves `theme` to local pages from now on: a page picks it up when it
    /// next loads. Every page reports its light or dark at once. Cheap when
    /// the colors did not change. Returns whether they changed from an
    /// earlier theme, so pages that hold the old colors want loading again.
    pub(crate) fn set_theme(&mut self, theme: &crate::config::Theme) -> bool {
        self.colors = Colors::from(theme);
        let key = super::kit::theme_key(theme);
        let first = self.theme.1.is_empty();
        if self.theme.0 == key && !first {
            return false;
        }
        self.theme = (key, super::kit::theme_css(theme));
        if let Some(engine) = Engine::started()
            && let Err(error) = engine.send(Command::Theme(self.theme.1.clone()))
        {
            tracing::debug!(%error, "The browser engine did not take the theme");
        }
        let light = super::kit::is_light(theme);
        if self.light != Some(light) {
            self.light = Some(light);
            for page in self.pages.values() {
                page.set_light(light);
            }
        }
        !first
    }

    /// Frees the textures of frames no page shows any more.
    pub(crate) fn free_frames(&self, window: &mut Window) {
        for image in self.retired.borrow_mut().drain(..) {
            let _ = window.drop_image(image);
        }
    }

    /// Shows the pages in `ids` and hides every other one; an empty list
    /// hides them all.
    pub(crate) fn present(&mut self, ids: &[TabId]) {
        let ids: Vec<TabId> = ids
            .iter()
            .copied()
            .filter(|id| self.pages.contains_key(id))
            .collect();
        if self.shown == ids {
            return;
        }
        for (id, page) in &self.pages {
            if ids.contains(id) {
                page.show();
            } else {
                page.hide();
            }
        }
        self.shown = ids;
    }

    pub(crate) fn close(&mut self, id: TabId) {
        if id == TabId::WALKR {
            self.walkr = None;
        }
        if id == TabId::WALKR_SESSION {
            self.walkr_session = None;
        }
        self.adopted.retain(|_, tab| *tab != id);
        self.shown.retain(|shown| *shown != id);
        self.drawn.borrow_mut().remove(&id);
        self.stopped.remove(&id);
        // Dropping the page closes its view on the engine's thread.
        self.pages.remove(&id);
    }

    /// Drops tab `id`'s page, which failed with `error`. A page whose
    /// script ended is remembered as stopped.
    pub(crate) fn fail(&mut self, id: TabId, error: &super::engine::Error) {
        self.close(id);
        if matches!(error, super::engine::Error::ScriptEnded) {
            self.stopped.insert(id);
        }
    }

    /// Whether tab `id`'s page stopped and was not made again.
    pub(crate) fn stopped(&self, id: TabId) -> bool {
        self.stopped.contains(&id)
    }

    /// Drops the pages of tabs the app no longer has.
    pub(crate) fn retain(&mut self, mut live: impl FnMut(TabId) -> bool) {
        let gone: Vec<_> = self.ids().filter(|id| !live(*id)).collect();
        for id in gone {
            self.close(id);
        }
        self.stopped.retain(|id| live(*id));
    }

    fn send(&self, id: TabId, command: impl FnOnce(PageKey) -> Command) {
        if let Some(page) = self.pages.get(&id) {
            page.send(command);
        }
    }

    /// Loads `location` in tab `id`'s page.
    pub(crate) fn load(&mut self, id: TabId, location: &Location) {
        self.send(id, |key| Command::Load(key, location.page_url()));
    }

    /// Runs one of the app's own scripts in the page. Never page-supplied
    /// text: data goes in as JSON.
    pub(crate) fn script(&self, id: TabId, script: &str) {
        self.send(id, |key| Command::Script(key, script.to_owned()));
    }

    /// Runs one of the app's own scripts in the page and hands what it
    /// answers to `done` as JSON, on the engine's thread. Whether the window
    /// has the page.
    pub(crate) fn evaluate(
        &self,
        id: TabId,
        script: &str,
        done: impl Fn(String) + Send + 'static,
    ) -> crate::Result<bool> {
        let Some(page) = self.pages.get(&id) else {
            return Ok(false);
        };
        // A script the engine did not take never answers.
        if page.send(|key| Command::Evaluate(key, script.to_owned(), Box::new(done))) {
            Ok(true)
        } else {
            Err(super::engine::Error::Busy.into())
        }
    }

    pub(crate) fn back(&self, id: TabId) {
        self.send(id, |key| Command::History(key, false));
    }

    pub(crate) fn forward(&self, id: TabId) {
        self.send(id, |key| Command::History(key, true));
    }

    pub(crate) fn reload(&self, id: TabId) {
        self.send(id, Command::Reload);
    }

    /// Servo pictures no part of a page yet, so no note gets a screenshot.
    pub(crate) fn capture(&self, _: TabId, _: super::annotate::Rect, _: u64) -> bool {
        false
    }

    /// The tab whose shown page holds the window's keyboard, if one does.
    pub(crate) fn keyboard_page(&self) -> Option<TabId> {
        self.shown
            .iter()
            .copied()
            .find(|id| self.pages.get(id).is_some_and(|page| page.holds_keyboard()))
    }

    /// Draws each page of `ids` at the zoom `zoom` gives its tab.
    pub(crate) fn follow_zoom(&self, ids: &[TabId], zoom: impl Fn(TabId) -> Option<Zoom>) {
        for id in ids {
            if let (Some(page), Some(zoom)) = (self.pages.get(id), zoom(*id)) {
                page.zoom(zoom);
            }
        }
    }

    /// Whether the window sent tab `id`'s page a key, pointer, or scroll
    /// input since this was last asked. The Walkr section counts it as
    /// activity.
    pub(crate) fn took_input(&self, id: TabId) -> bool {
        self.pages.get(&id).is_some_and(|page| page.took_input())
    }

    /// Does what each page asked of the clipboard, in a window that is
    /// `active`; an inactive one does nothing of it.
    fn serve_clipboard(&self, active: bool, cx: &gpui::App) {
        for page in self.pages.values() {
            page.serve_clipboard(active, cx);
        }
    }

    /// Hands the keyboard back from the page to the window.
    pub(crate) fn blur(&self, id: TabId) {
        if let Some(page) = self.pages.get(&id) {
            page.blur();
        }
    }

    /// Moves keyboard input into the shown page.
    pub(crate) fn focus(&self, id: TabId) {
        if let Some(page) = self.pages.get(&id) {
            page.focus();
        }
    }

    /// The next thing the pages reported.
    pub(crate) fn next_event(&self) -> Option<Event> {
        while let Ok(event) = self.events.try_recv() {
            let resolved = event.resolve(|source| match source {
                Source::Tab(id) => Some(id),
                Source::Walkr(key) => (self.walkr == Some(key)).then_some(TabId::WALKR),
                Source::WalkrSession(key) => {
                    (self.walkr_session == Some(key)).then_some(TabId::WALKR_SESSION)
                }
                Source::Popup(key) => self.adopted.get(&key).copied(),
            });
            if resolved.is_some() {
                return resolved;
            }
        }
        // A page whose script stopped says so in its mailbox, which no
        // full queue loses ([`super::engine`]), and so does a popup's page
        // that the engine ended as its tab took it.
        self.pages.iter().find_map(|(id, page)| {
            if page.mailbox().refused.take() {
                return Some(Event::Closed(*id));
            }
            let stopped = page.mailbox().stopped.take();
            stopped.then_some(Event::Failed(*id, super::engine::Error::ScriptEnded))
        })
    }
}

impl SheprdWindow {
    /// The window stopped being the active one: no page of it keeps a
    /// grant to use the clipboard.
    pub(crate) fn end_page_clipboard_grants(&self) {
        for page in self.browser.pages.pages.values() {
            page.end_clipboard_grants();
        }
    }

    /// The window stopped being the active one: the page with its keyboard
    /// holds no modifier key, as it hears no release from now on.
    pub(crate) fn release_page_keys(&self) {
        let pages = self.browser.pages.pages.values();
        for page in pages.filter(|page| page.holds_keyboard()) {
            page.hold(gpui::Modifiers::default());
        }
    }
}

#[cfg(test)]
mod tests;
