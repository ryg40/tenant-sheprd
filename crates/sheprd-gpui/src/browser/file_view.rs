//! What surrounds a local file a tab shows: a breadcrumb of its path from
//! the workspace's folder, the switch between the rendered page and its
//! source for Markdown and HTML files, the way to the editor, and following
//! the file as it changes on disk.
//!
//! A local page keeps its `Location::Local` while its source shows, so its
//! page stays alive, hidden, with the folder it may read. The source is the
//! read-only code view. The switch is the window's own, like its groups: it
//! is not saved with the tabs.
//!
//! File names are untrusted: a breadcrumb drops control and direction
//! characters, as the code view does for a file's text.

mod choice;
mod render;

use super::{Location, Store, Tab, TabId, groups::Shown, view::store};
use crate::{SheprdWindow, code_view::CodeFile, control::SourceWhy, i18n::fl, window::Flash};
pub(crate) use choice::{FileView, browser_view, is_markdown, renders, view_for};
use gpui::{prelude::*, *};
use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

/// How often the shown files' metadata is read.
const POLL: Duration = Duration::from_secs(1);
/// Files read per poll at most: the tabs the window's groups show.
const MAX_WATCHED: usize = 16;
/// Names a breadcrumb shows at most; a deeper path loses its middle.
const MAX_CRUMBS: usize = 6;
/// What stands for the middle a deep path's breadcrumb lost.
const GAP: &str = "\u{2026}";

/// What tells one version of a file from the next, without reading it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

impl Stamp {
    /// Blocking: it runs on the background executor.
    fn read(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            modified: metadata.modified().ok(),
            len: metadata.len(),
        })
    }
}

/// One window's file views.
#[derive(Default)]
pub(crate) struct FileViews {
    /// The local pages that show their source.
    source: HashSet<TabId>,
    /// Each shown file as last read, by its tab.
    stamps: HashMap<TabId, (PathBuf, Stamp)>,
    checked: Option<Instant>,
    /// Whether a read is on its way; the next waits for it.
    checking: bool,
}

impl FileViews {
    /// Whether `tab` is a local page that shows its source. A tab that
    /// left its file for a web address shows that page, whatever it showed
    /// before.
    pub(crate) fn shows_source(&self, tab: &Tab) -> bool {
        matches!(tab.location, Some(Location::Local { .. })) && self.source.contains(&tab.id)
    }

    /// The tabs of `read` that now show another file than when last read.
    fn moved(&self, read: &[(TabId, PathBuf, Option<Stamp>)]) -> Vec<TabId> {
        read.iter()
            .filter(|(id, path, _)| self.stamps.get(id).is_some_and(|(old, _)| old != path))
            .map(|(id, _, _)| *id)
            .collect()
    }

    /// Records what was read of each tab's file. Returns the tabs whose
    /// file changed since it was last read. A file first seen, a tab that
    /// went to another file, and a file that is missing change nothing: the
    /// old page stays until there is a new one to show.
    pub(super) fn changed(
        &mut self,
        read: impl IntoIterator<Item = (TabId, PathBuf, Option<Stamp>)>,
    ) -> Vec<TabId> {
        let mut changed = Vec::new();
        for (id, path, stamp) in read {
            let Some(stamp) = stamp else {
                continue;
            };
            if let Some((old_path, old)) = self.stamps.get(&id)
                && *old_path == path
                && *old != stamp
            {
                changed.push(id);
            }
            self.stamps.insert(id, (path, stamp));
        }
        changed
    }
}

/// The local file `tab` shows, as a page or as code.
fn tab_file(tab: &Tab) -> Option<PathBuf> {
    match tab.location.as_ref()? {
        Location::Local { file } => Some(file.path()),
        Location::Code { file } => Some(PathBuf::from(&file.path)),
        _ => None,
    }
}

/// The names a breadcrumb of `path` shows: from the workspace's folder
/// `root` when the file is in it, else from the home folder, else the whole
/// path. A deep path keeps its first name and its last ones.
pub(super) fn crumbs(path: &Path, root: Option<&Path>, home: Option<&Path>) -> Vec<String> {
    let clean = |name: &std::ffi::OsStr| {
        let mut cleaned = String::new();
        crate::review::push_clean(&mut cleaned, &name.to_string_lossy());
        cleaned
    };
    let names = |path: &Path| -> Vec<String> {
        path.components()
            .filter_map(|part| match part {
                Component::Normal(name) => Some(clean(name)),
                _ => None,
            })
            .collect()
    };
    let inside = |folder: Option<&Path>| path.strip_prefix(folder?).ok();
    let mut crumbs = if let (Some(root), Some(rest)) = (root, inside(root)) {
        let mut crumbs: Vec<String> = root.file_name().map(clean).into_iter().collect();
        crumbs.extend(names(rest));
        crumbs
    } else if let Some(rest) = inside(home) {
        let mut crumbs = vec!["~".to_owned()];
        crumbs.extend(names(rest));
        crumbs
    } else {
        names(path)
    };
    if crumbs.len() > MAX_CRUMBS {
        let tail = crumbs.split_off(crumbs.len() - (MAX_CRUMBS - 2));
        crumbs.truncate(1);
        crumbs.push(GAP.to_owned());
        crumbs.extend(tail);
    }
    crumbs
}

/// The folder that name `index` of the breadcrumb `names` of `path` stands
/// for, which a click shows in the file explorer. `None` for the file's own
/// name, and for the first name and the gap of a path that lost its middle.
pub(super) fn crumb_folder<'a>(path: &'a Path, names: &[String], index: usize) -> Option<&'a Path> {
    let last = names.len().checked_sub(1)?;
    let cut = names.len() == MAX_CRUMBS && names.get(1).is_some_and(|name| name == GAP);
    if index >= last || (cut && index <= 1) {
        return None;
    }
    path.ancestors().nth(last - index)
}

/// Why a file shows only its source, as its bar and a refused switch say.
pub(super) fn source_only(why: SourceWhy) -> String {
    match why {
        SourceWhy::Build => fl!("browser-file-source-only-build"),
        SourceWhy::Asked | SourceWhy::Folder => fl!("browser-file-source-only-folder"),
    }
}

impl SheprdWindow {
    /// The file tab `id` shows as code: a code tab's own, or a local page's
    /// while its source shows.
    pub(crate) fn code_file(&self, id: TabId, cx: &App) -> Option<CodeFile> {
        let tab = cx.try_global::<Store>()?.get(id)?;
        match tab.location.as_ref()? {
            Location::Code { file } => Some(file.clone()),
            Location::Local { file } if self.browser.file_views.shows_source(tab) => {
                Some(CodeFile {
                    path: file.path().to_str()?.to_owned(),
                    line: None,
                })
            }
            _ => None,
        }
    }

    /// Whether tab `id` is a local page that shows its source.
    pub(crate) fn shows_file_source(&self, id: TabId, cx: &App) -> bool {
        store(cx)
            .and_then(|store| store.get(id))
            .is_some_and(|tab| self.browser.file_views.shows_source(tab))
    }

    /// Shows tab `id`'s file as its source, or as the rendered page. A code
    /// tab of a Markdown or HTML file becomes that file's page. `focus` is
    /// whether the view takes the keyboard: a request that leaves the focus
    /// alone leaves it here too.
    pub(crate) fn show_file_source(
        &mut self,
        id: TabId,
        source: bool,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.file_shown_as(id, source, focus, super::EMBEDDED, window, cx);
    }

    /// [`Self::show_file_source`], on a build that shows pages inside the
    /// window or not (`embedded`). One that shows none refuses the rendered
    /// page and says why; the tab stays on its source.
    pub(super) fn file_shown_as(
        &mut self,
        id: TabId,
        source: bool,
        focus: bool,
        embedded: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = store(cx).and_then(|store| store.get(id)).cloned() else {
            return;
        };
        match (&tab.location, source) {
            (Some(Location::Local { .. }), true) => {
                if self.browser.file_views.source.insert(id) {
                    self.show_code_view(id, cx);
                }
                self.focus_code_view(id, focus, window, cx);
            }
            (Some(Location::Code { .. }), true) => {
                self.show_code_view(id, cx);
                self.focus_code_view(id, focus, window, cx);
            }
            (Some(Location::Local { .. }), false) => {
                if !self.browser.file_views.source.contains(&id) {
                    return;
                }
                if !embedded {
                    self.show_flash(Flash::warning(source_only(SourceWhy::Build)), cx);
                    return;
                }
                self.browser.file_views.source.remove(&id);
                self.code_views.remove(&id);
                // The file may have changed while its source showed.
                #[cfg(embedded_pages)]
                self.browser.pages.reload(id);
                if focus {
                    window.focus(&self.focus, cx);
                }
            }
            (Some(Location::Code { file }), false) if renders(Path::new(&file.path)) => {
                let home = crate::config::home().ok();
                match view_for(Path::new(&file.path), home.as_deref(), embedded) {
                    Ok(FileView::Page(file)) => {
                        self.code_views.remove(&id);
                        Store::update(cx, |store| store.asked(id, Location::Local { file }));
                        if focus {
                            window.focus(&self.focus, cx);
                        }
                    }
                    Ok(FileView::Source(_, why)) => {
                        self.show_flash(Flash::warning(source_only(why)), cx);
                        return;
                    }
                    Ok(FileView::Code(_)) => return,
                    Err(error) => {
                        let why = crate::i18n::Localize::localize(&error);
                        self.show_flash(Flash::warning(why), cx);
                        return;
                    }
                }
            }
            _ => return,
        }
        cx.notify();
    }

    /// Gives the keyboard to tab `id`'s code view, when `focus` asks.
    fn focus_code_view(&self, id: TabId, focus: bool, window: &mut Window, cx: &mut App) {
        if focus && let Some(view) = self.code_views.get(&id) {
            window.focus(&view.focus, cx);
        }
    }

    /// Switches the file the group in use shows between its page and its
    /// source, for the `toggle_source` binding.
    pub(crate) fn toggle_shown_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self
            .active_group()
            .and_then(|group| match self.group_shown(group, cx) {
                Shown::Page(id) => store(cx)?.get(id).cloned(),
                _ => None,
            })
            .filter(|tab| tab_file(tab).is_some_and(|path| renders(&path)));
        let Some(tab) = shown else {
            self.show_flash(Flash::warning(fl!("browser-file-no-switch")), cx);
            return;
        };
        let source = matches!(tab.location, Some(Location::Local { .. }))
            && !self.browser.file_views.shows_source(&tab);
        self.show_file_source(tab.id, source, true, window, cx);
    }

    /// Takes the window's theme to its pages. A Markdown page has the
    /// colors written into it as it is served, so the open ones load again
    /// when the theme changes; any other page follows by itself.
    #[cfg(embedded_pages)]
    pub(crate) fn theme_pages(&mut self, cx: &App) {
        if !self.browser.pages.set_theme(&self.theme) {
            return;
        }
        let Some(store) = store(cx) else {
            return;
        };
        for tab in store.tabs() {
            if let Some(Location::Local { file }) = &tab.location
                && is_markdown(&file.path())
            {
                self.browser.pages.reload(tab.id);
            }
        }
    }

    /// Reads tab `id`'s file again: its source, or its page.
    pub(super) fn reload_file_view(&mut self, id: TabId, cx: &mut Context<Self>) {
        if self.code_views.contains_key(&id) {
            self.reload_code_view(id, cx);
            return;
        }
        #[cfg(embedded_pages)]
        self.browser.pages.reload(id);
        #[cfg(not(embedded_pages))]
        let _ = cx;
    }

    /// Runs every window tick: now and then reads the metadata of the files
    /// the window's groups show, off the UI thread, and reloads the views of
    /// those that changed. A page reloads where it is; the code view keeps
    /// its place.
    pub(super) fn poll_file_views(&mut self, cx: &mut Context<Self>) {
        let views = &self.browser.file_views;
        if views.checking || views.checked.is_some_and(|at| at.elapsed() < POLL) {
            return;
        }
        let Some(store) = store(cx) else {
            return;
        };
        let watched: Vec<(TabId, PathBuf)> = self
            .group_slots()
            .into_iter()
            .filter_map(|slot| match self.group_shown(slot.id, cx) {
                Shown::Page(id) => Some((id, tab_file(store.get(id)?)?)),
                _ => None,
            })
            .take(MAX_WATCHED)
            .collect();
        // A tab that left its file, as for a web address, is no longer one
        // that shows its source; nor does it become one when it returns.
        let local = |id: &TabId| {
            store
                .get(*id)
                .is_some_and(|tab| matches!(tab.location, Some(Location::Local { .. })))
        };
        let code = |id: &TabId| {
            store
                .get(*id)
                .is_some_and(|tab| matches!(tab.location, Some(Location::Code { .. })))
        };
        let views = &mut self.browser.file_views;
        views.source.retain(local);
        views.stamps.retain(|id, _| store.get(*id).is_some());
        let source = &views.source;
        self.code_views
            .retain(|id, _| source.contains(id) || code(id));
        views.checked = Some(Instant::now());
        if watched.is_empty() {
            return;
        }
        views.checking = true;
        let read = cx.background_executor().spawn(async move {
            watched
                .into_iter()
                .map(|(id, path)| {
                    let stamp = Stamp::read(&path);
                    (id, path, stamp)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let read = read.await;
            let _ = this.update(cx, |this, cx| {
                this.browser.file_views.checking = false;
                // A page that went to another file while its source showed:
                // the source follows it.
                for id in this.browser.file_views.moved(&read) {
                    if this.code_views.contains_key(&id) {
                        this.reload_code_view(id, cx);
                    }
                }
                for id in this.browser.file_views.changed(read) {
                    this.reload_file_view(id, cx);
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests;
