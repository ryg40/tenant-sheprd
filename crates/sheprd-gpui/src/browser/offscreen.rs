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
    collections::HashMap,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
};

/// Pages are numbered across windows, so a key never names two.
static NEXT_PAGE: AtomicU64 = AtomicU64::new(0);

pub(crate) struct Pages {
    /// The theme's colors the pages' own lists are drawn in.
    colors: Colors,
    pages: HashMap<TabId, Rc<Page>>,
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
        let Some(location) = tab
            .location
            .as_ref()
            .filter(|location| location.is_page())
            .filter(|_| !self.pages.contains_key(&tab.id))
        else {
            return Ok(());
        };
        let engine = Engine::get()?;
        if self.redraw.is_none() {
            let notifier = self.notifier.clone();
            self.redraw = Some(cx.spawn(async move |this, cx| {
                loop {
                    notifier.notified().await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            }));
            engine.send(Command::Theme(self.theme.1.clone()))?;
        }
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
        let key: PageKey = NEXT_PAGE.fetch_add(1, Ordering::Relaxed);
        let (mailbox, latest) = (Arc::new(Mailbox::default()), Arc::new(Latest::default()));
        // An engine that did not take the page leaves the tab without one,
        // so showing the tab again asks again.
        engine.send(Command::Open(Box::new(Open {
            key,
            url: location.page_url(),
            // Only a local page, which the agent wrote, gets `herdr.send`.
            bridge: root.is_some(),
            root,
            light: self.light,
            source: Source::Tab(tab.id),
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
            (self.reports.clone(), Source::Tab(tab.id)),
            cx.focus_handle(),
            self.retired.clone(),
        );
        self.pages.insert(tab.id, Rc::new(page));
        Ok(())
    }

    /// Serves `theme` to local pages from now on: a page picks it up when it
    /// next loads. Every page reports its light or dark at once. Cheap when
    /// the colors did not change.
    pub(crate) fn set_theme(&mut self, theme: &crate::config::Theme) {
        self.colors = Colors::from(theme);
        let key = super::kit::theme_key(theme);
        if self.theme.0 == key && !self.theme.1.is_empty() {
            return;
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
        self.shown.retain(|shown| *shown != id);
        self.drawn.borrow_mut().remove(&id);
        // Dropping the page closes its view on the engine's thread.
        self.pages.remove(&id);
    }

    /// Drops the pages of tabs the app no longer has.
    pub(crate) fn retain(&mut self, mut live: impl FnMut(TabId) -> bool) {
        let gone: Vec<_> = self.ids().filter(|id| !live(*id)).collect();
        for id in gone {
            self.close(id);
        }
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
        let event = self.events.try_recv().ok()?;
        event.resolve(|source| match source {
            Source::Tab(id) => Some(id),
        })
    }
}
