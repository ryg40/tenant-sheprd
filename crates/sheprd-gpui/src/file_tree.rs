//! The file explorer: a panel on the window's right with the folders of the
//! focused pane's checkout as a tree, as an editor's explorer. A folder's
//! entries are read when it is opened, on the background executor (see
//! `sheprd_vcs::file_tree`), and the tree is drawn from rows prepared when
//! a listing lands, so drawing reads no disk. Typing in the search field
//! lists files by name instead, ranked by Go to File's index.
//!
//! Choosing a file opens it the way `sheprd browser open` and `sheprd code
//! open` do: a Markdown or HTML file as a page, any other file in a
//! read-only code tab. Only a local checkout is listed.
// i18n: migrated

mod open;
mod render;
mod rows;
mod search;

use crate::{
    SheprdWindow,
    i18n::{Localize, fl},
    motion,
    search_input::{Changed, SearchInput},
};
use gpui::{prelude::*, *};
use rows::Row;
use search::Search;
use sheprd_vcs::file_tree::{self, Listing};
use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// The most folders open at once, which bounds a refresh, the rows, and
/// the folders kept: a folder's entries are kept only while it is open.
const MAX_OPEN: usize = 256;
/// The most folders read at once. A refresh reads every open folder, and
/// the rest wait in line, so the app's other background work keeps going.
const MAX_READS: usize = 4;
/// The most roots whose open folders are kept for when the tree comes
/// back to them; the one left longest ago is forgotten first.
const MAX_KEPT_ROOTS: usize = 16;

/// Where the tree's root stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Root {
    /// No pane is focused, or it reports no folder.
    None,
    Locating,
    Ready(PathBuf),
    /// The focused pane runs on another host.
    Remote,
    Failed(String),
}

/// One folder's entries, as far as they are read.
#[derive(Debug)]
enum Folder {
    Loading,
    Loaded(Arc<Listing>),
    Failed(String),
}

/// The folder reads asked for. Each has a number, and only the newest of a
/// folder still shown is taken, so a slow earlier read cannot replace a
/// later one, and a read of a folder closed since is dropped.
#[derive(Debug, Default)]
struct Reads {
    /// The newest request of each folder whose answer is still wanted.
    latest: HashMap<String, u64>,
    next: u64,
    /// Requests not started: at most one per folder.
    queue: VecDeque<(String, u64)>,
    running: usize,
}

impl Reads {
    /// A new request for `path`, in line; every earlier one of it is stale.
    fn ask(&mut self, path: &str) -> u64 {
        self.next += 1;
        self.latest.insert(path.to_owned(), self.next);
        self.queue.retain(|(queued, _)| queued != path);
        self.queue.push_back((path.to_owned(), self.next));
        self.next
    }

    fn current(&self, path: &str, request: u64) -> bool {
        self.latest.get(path) == Some(&request)
    }

    /// No answer for `path` is wanted any more.
    fn forget(&mut self, path: &str) {
        self.latest.remove(path);
        self.queue.retain(|(queued, _)| queued != path);
    }
}

pub(crate) struct FileTree {
    /// Stops the reads of a root nobody shows any more.
    cancelled: Arc<AtomicBool>,
    /// The folder the root was found from: the focused pane's.
    cwd: Option<String>,
    /// Open folders, by path relative to the root.
    expanded: BTreeSet<String>,
    pub(crate) focus: FocusHandle,
    folders: HashMap<String, Folder>,
    /// The open folders of roots the tree left, for this session: the
    /// root left last is at the back. A root's folders leave it when the
    /// tree shows that root again.
    kept: VecDeque<(PathBuf, BTreeSet<String>)>,
    motion: motion::Toggle,
    /// The user's choice, for this window and session.
    open: bool,
    /// Whether the motion was set yet: the first setting does not slide.
    placed: bool,
    reads: Reads,
    /// A file to bring into view once the folders holding it are read.
    reveal: Option<String>,
    root: Root,
    rows: Vec<Row>,
    scroll: UniformListScrollHandle,
    search: Entity<SearchInput>,
    searching: Search,
    /// The selected row, by path.
    selected: Option<String>,
    /// Counts roots, so a read of an earlier one is dropped.
    serial: u64,
    /// A folder to show once the root is found: one asked for while the
    /// panel was hidden or still looking for its root.
    wanted: Option<PathBuf>,
    _subscription: Subscription,
}

impl Drop for FileTree {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

impl FileTree {
    pub(crate) fn new(cx: &mut Context<SheprdWindow>) -> Self {
        let search = cx.new(SearchInput::new);
        let subscription = cx.subscribe(&search, |this, search, _: &Changed, cx| {
            let query = search.read(cx).text().to_owned();
            this.search_files(query, cx);
        });
        search.update(cx, |input, cx| {
            input.set_placeholder_with(|| fl!("file-tree-search"), cx);
            input.set_frameless(cx);
        });
        Self {
            cancelled: Arc::default(),
            cwd: None,
            expanded: BTreeSet::new(),
            focus: cx.focus_handle(),
            folders: HashMap::new(),
            kept: VecDeque::new(),
            motion: motion::Toggle::default(),
            open: false,
            placed: false,
            reads: Reads::default(),
            reveal: None,
            root: Root::None,
            rows: Vec::new(),
            scroll: UniformListScrollHandle::new(),
            search,
            searching: Search::default(),
            selected: None,
            serial: 0,
            wanted: None,
            _subscription: subscription,
        }
    }

    /// Whether the user has the panel open.
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn root(&self) -> &Root {
        &self.root
    }

    fn ready_root(&self) -> Option<&Path> {
        match &self.root {
            Root::Ready(root) => Some(root),
            _ => None,
        }
    }

    /// Whether the root is still being found, so a request for it waits.
    pub(crate) fn settling(&self) -> bool {
        self.root == Root::Locating
    }

    /// Forgets everything read under the root, and stops its reads. The
    /// open folders of the root left are kept, and those kept for `root`
    /// are open again, for the caller to read.
    fn reset(&mut self, root: Root) {
        self.cancelled.store(true, Ordering::Release);
        self.cancelled = Arc::default();
        self.serial += 1;
        // Taken before the root left is kept, so the bound on kept roots
        // never drops the one the tree comes back to.
        let restored = match &root {
            Root::Ready(root) => self
                .kept
                .iter()
                .position(|(kept, _)| kept == root)
                .and_then(|at| self.kept.remove(at))
                .map(|(_, open)| open),
            _ => None,
        };
        let open = std::mem::replace(&mut self.expanded, restored.unwrap_or_default());
        if let Root::Ready(left) = std::mem::replace(&mut self.root, root) {
            self.kept.retain(|(kept, _)| *kept != left);
            if !open.is_empty() {
                self.kept.push_back((left, open));
            }
            if self.kept.len() > MAX_KEPT_ROOTS {
                self.kept.pop_front();
            }
        }
        self.folders.clear();
        self.reads = Reads::default();
        self.reveal = None;
        self.selected = None;
        // A folder that waited for the root this one replaces is not asked
        // of the next root: its click was made under another folder.
        self.wanted = None;
        self.searching.reset();
        self.rebuild();
    }

    /// The panel's slide, for the layout of the window's right-hand columns.
    pub(crate) fn motion(&self) -> &motion::Toggle {
        &self.motion
    }

    /// The search field, which takes the window's font and theme when drawn.
    pub(crate) fn search(&self) -> &Entity<SearchInput> {
        &self.search
    }

    /// Closes folder `path` and forgets what was read of it. `under` also
    /// closes the folders below it, as when it is gone from the disk.
    fn close(&mut self, path: &str, under: bool) {
        let below = format!("{path}/");
        let closes = |open: &str| open == path || (under && open.starts_with(&below));
        self.expanded.retain(|open| !closes(open));
        self.folders.retain(|open, _| !closes(open));
        let gone: Vec<String> = self
            .reads
            .latest
            .keys()
            .filter(|open| closes(open))
            .cloned()
            .collect();
        for path in gone {
            self.reads.forget(&path);
        }
    }

    fn rebuild(&mut self) {
        self.rows = match (&self.searching.hits, self.searching.query.is_empty()) {
            (Some(hits), false) => rows::found(hits),
            _ => rows::tree(&self.folders, &self.expanded),
        };
        let Some(reveal) = &self.reveal else {
            return;
        };
        if let Some(row) = self.rows.iter().position(|row| row.path == *reveal) {
            self.scroll.scroll_to_item(row, ScrollStrategy::Center);
            self.reveal = None;
        }
    }

    fn selected_row(&self) -> Option<usize> {
        let selected = self.selected.as_ref()?;
        self.rows.iter().position(|row| row.path == *selected)
    }

    fn select(&mut self, row: usize) {
        if let Some(found) = self.rows.get(row) {
            self.selected = Some(found.path.clone());
            self.scroll.scroll_to_item(row, ScrollStrategy::Center);
        }
    }

    /// The rows shown, for tests: each path, and whether it is a folder.
    #[cfg(test)]
    pub(crate) fn shown(&self) -> Vec<(&str, bool)> {
        self.rows
            .iter()
            .map(|row| {
                (
                    row.path.as_str(),
                    matches!(row.what, rows::What::Folder { .. }),
                )
            })
            .collect()
    }
}

impl SheprdWindow {
    /// The folder of the focused pane, which the tree's root is found from.
    fn file_tree_cwd(&self) -> Option<String> {
        let snapshot = self.live.snapshot.as_deref()?;
        let id = snapshot.focused_pane_id.as_deref()?;
        let pane = snapshot.panes.iter().find(|pane| pane.pane_id == id)?;
        pane.foreground_cwd.clone().or_else(|| pane.cwd.clone())
    }

    /// Shows or hides the panel for the user, who asked in this window:
    /// showing also gives the panel the keyboard.
    pub(crate) fn show_file_tree(
        &mut self,
        show: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_tree.open == show {
            return;
        }
        self.set_file_tree_shown(show, window, cx);
        if show {
            window.focus(&self.file_tree.focus, cx);
        }
    }

    /// Shows or hides the panel. Showing reads the tree again and leaves
    /// the keyboard where it is, so a command an agent runs does not take
    /// the user's typing; hiding returns the keyboard to the terminal when
    /// the panel had it.
    pub(crate) fn set_file_tree_shown(
        &mut self,
        show: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_tree.open == show {
            return;
        }
        self.file_tree.open = show;
        if show {
            self.file_tree.cwd = None;
            self.poll_file_tree(cx);
            self.refresh_file_tree(cx);
        } else if self.file_tree.focus.contains_focused(window, cx)
            || self.file_tree.search.read(cx).focus.is_focused(window)
        {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// Follows the focused pane: a pane in another folder may be in
    /// another checkout, which is found on the background executor. Runs
    /// on every window tick, and does nothing while the panel is hidden.
    pub(crate) fn poll_file_tree(&mut self, cx: &mut Context<Self>) {
        if !self.file_tree.open {
            return;
        }
        if self.selected_is_remote() {
            if self.file_tree.root != Root::Remote {
                self.file_tree.cwd = None;
                self.file_tree.reset(Root::Remote);
                cx.notify();
            }
            return;
        }
        let cwd = self.file_tree_cwd();
        if cwd == self.file_tree.cwd && self.file_tree.root != Root::Remote {
            return;
        }
        self.file_tree.cwd = cwd.clone();
        // A folder that waited for the last folder's root is not asked of
        // this one's.
        self.file_tree.wanted = None;
        let Some(cwd) = cwd else {
            self.file_tree.reset(Root::None);
            cx.notify();
            return;
        };
        if !matches!(self.file_tree.root, Root::Ready(_)) {
            self.file_tree.root = Root::Locating;
            cx.notify();
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let asked = cwd.clone();
        let found = cx
            .background_executor()
            .spawn(async move { file_tree::root(&cwd, &cancelled) });
        cx.spawn(async move |this, cx| {
            let found = found.await;
            let _ = this.update(cx, |this, cx| {
                // A newer pane's folder is already being looked at.
                if this.file_tree.cwd.as_deref() != Some(asked.as_str()) {
                    return;
                }
                let wanted = this.file_tree.wanted.take();
                match found {
                    Ok(root) if this.file_tree.ready_root() == Some(&root) => {}
                    Ok(root) => {
                        this.file_tree.reset(Root::Ready(root));
                        this.read_open_folders(cx);
                        let query = std::mem::take(&mut this.file_tree.searching.query);
                        this.search_files(query, cx);
                    }
                    Err(error) => this
                        .file_tree
                        .reset(Root::Failed(crate::Error::from(error).localize())),
                }
                if let Some(folder) = wanted {
                    this.reveal_folder(&folder, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Reads folder `path` of the root, once one of the [`MAX_READS`] reads
    /// is free. What was listed before shows until the new listing lands.
    fn load_folder(&mut self, path: String, cx: &mut Context<Self>) {
        let tree = &mut self.file_tree;
        if tree.ready_root().is_none() {
            return;
        }
        tree.folders.entry(path.clone()).or_insert(Folder::Loading);
        tree.reads.ask(&path);
        self.start_folder_reads(cx);
    }

    /// Starts the reads in line, as far as [`MAX_READS`] allows.
    fn start_folder_reads(&mut self, cx: &mut Context<Self>) {
        loop {
            let tree = &mut self.file_tree;
            let Some(root) = tree.ready_root().map(Path::to_owned) else {
                return;
            };
            if tree.reads.running >= MAX_READS {
                return;
            }
            let Some((path, request)) = tree.reads.queue.pop_front() else {
                return;
            };
            tree.reads.running += 1;
            let (cancelled, serial) = (tree.cancelled.clone(), tree.serial);
            let folder = path.clone();
            let listed = cx
                .background_executor()
                .spawn(async move { file_tree::list(&root, &folder, &cancelled) });
            cx.spawn(async move |this, cx| {
                let listed = listed.await;
                let _ = this.update(cx, |this, cx| {
                    // Another root counts its own reads.
                    if this.file_tree.serial != serial {
                        return;
                    }
                    let reads = &mut this.file_tree.reads;
                    reads.running = reads.running.saturating_sub(1);
                    this.folder_listed(path, request, listed, cx);
                    this.start_folder_reads(cx);
                });
            })
            .detach();
        }
    }

    /// Takes the answer to request `request` for folder `path`, unless a
    /// newer request for the folder exists or the folder was closed. A
    /// folder that is gone closes, with the folders below it.
    fn folder_listed(
        &mut self,
        path: String,
        request: u64,
        listed: sheprd_vcs::Result<Listing>,
        cx: &mut Context<Self>,
    ) {
        let tree = &mut self.file_tree;
        if !tree.reads.current(&path, request) {
            return;
        }
        tree.reads.forget(&path);
        match listed {
            Ok(listing) => {
                tree.folders.insert(path, Folder::Loaded(Arc::new(listing)));
            }
            Err(sheprd_vcs::Error::FileTree(file_tree::Error::NotFolder)) if !path.is_empty() => {
                tree.close(&path, true);
            }
            Err(error) => {
                let failed = Folder::Failed(crate::Error::from(error).localize());
                tree.folders.insert(path, failed);
            }
        }
        tree.rebuild();
        cx.notify();
    }

    /// Reads the root and every open folder. A folder gone from the disk
    /// closes when its read answers.
    fn read_open_folders(&mut self, cx: &mut Context<Self>) {
        let open: Vec<String> = self.file_tree.expanded.iter().cloned().collect();
        self.load_folder(String::new(), cx);
        for path in open {
            self.load_folder(path, cx);
        }
    }

    /// Reads the root and every open folder again, and the search's index
    /// while a search shows.
    pub(crate) fn refresh_file_tree(&mut self, cx: &mut Context<Self>) {
        if self.file_tree.ready_root().is_none() {
            return;
        }
        self.read_open_folders(cx);
        if !self.file_tree.searching.query.is_empty() {
            self.rank_files(true, cx);
        }
        cx.notify();
    }

    /// Opens or closes folder `path`.
    fn toggle_folder(&mut self, path: &str, cx: &mut Context<Self>) {
        let tree = &mut self.file_tree;
        if tree.expanded.contains(path) {
            // The folders open below it stay so, for when it opens again.
            tree.close(path, false);
            tree.rebuild();
            cx.notify();
            return;
        }
        if tree.expanded.len() >= MAX_OPEN {
            self.show_flash(
                crate::window::Flash::warning(fl!("file-tree-too-many-open")),
                cx,
            );
            return;
        }
        tree.expanded.insert(path.to_owned());
        tree.rebuild();
        self.load_folder(path.to_owned(), cx);
        cx.notify();
    }

    /// Closes every folder.
    fn collapse_file_tree(&mut self, cx: &mut Context<Self>) {
        let tree = &mut self.file_tree;
        let open: Vec<String> = tree.expanded.iter().cloned().collect();
        for path in open {
            tree.close(&path, false);
        }
        tree.rebuild();
        cx.notify();
    }

    /// Shows the panel with the file at absolute `path` selected and its
    /// folders open; the path relative to the root, or why it is refused.
    pub(crate) fn reveal_file(
        &mut self,
        path: &Path,
        cx: &mut Context<Self>,
    ) -> crate::Result<String> {
        self.reveal(path, false, cx)
    }

    /// Selects the row at absolute `path` with the folders holding it open,
    /// and the row itself open when `open` says it is a folder, so the bound
    /// on open folders counts it.
    fn reveal(&mut self, path: &Path, open: bool, cx: &mut Context<Self>) -> crate::Result<String> {
        let Some(root) = self.file_tree.ready_root() else {
            return Err(sheprd_vcs::Error::from(file_tree::Error::NotFolder).into());
        };
        let relative = file_tree::locate(root, path).map_err(crate::Error::from)?;
        let search = self.file_tree.search.clone();
        search.update(cx, |input, cx| input.set_text_at("", 0, cx));
        self.search_files(String::new(), cx);
        let mut folder = String::new();
        let mut folders = Vec::new();
        if let Some((parents, _)) = relative.rsplit_once('/') {
            for name in parents.split('/') {
                if !folder.is_empty() {
                    folder.push('/');
                }
                folder.push_str(name);
                folders.push(folder.clone());
            }
        }
        if open && !relative.is_empty() {
            folders.push(relative.clone());
        }
        if folders.len() + self.file_tree.expanded.len() > MAX_OPEN {
            self.file_tree.expanded.clear();
        }
        for folder in folders {
            if self.file_tree.expanded.insert(folder.clone())
                || !self.file_tree.folders.contains_key(&folder)
            {
                self.load_folder(folder, cx);
            }
        }
        self.file_tree.selected = Some(relative.clone());
        self.file_tree.reveal = Some(relative.clone());
        self.file_tree.rebuild();
        cx.notify();
        Ok(relative)
    }

    /// Opens the folder at absolute `path` in the tree, selected, with the
    /// folders holding it open. A folder the tree does not list is refused
    /// in a flash.
    fn reveal_folder(&mut self, path: &Path, cx: &mut Context<Self>) {
        if self.file_tree.ready_root().is_none() {
            return;
        }
        match self.reveal(path, true, cx) {
            // The root itself is always open, and has no row to select.
            Ok(folder) if folder.is_empty() => {
                self.file_tree.selected = None;
                self.file_tree.reveal = None;
            }
            Ok(_) => {}
            Err(error) => self.show_flash(crate::window::Flash::warning(error.localize()), cx),
        }
    }

    /// Shows the panel with the folder at absolute `path` open, for a
    /// click on a folder of a file's breadcrumb. The keyboard stays where
    /// it is. A hidden panel finds its root first, off the UI thread.
    pub(crate) fn show_folder_in_file_tree(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shown = self.file_tree.open;
        self.set_file_tree_shown(true, window, cx);
        if shown && self.file_tree.ready_root().is_some() {
            self.reveal_folder(&path, cx);
        } else if self.file_tree.settling() || (!shown && self.file_tree.cwd.is_some()) {
            self.file_tree.wanted = Some(path);
        }
    }

    /// The folder the tree lists, or will list once shown: where a
    /// breadcrumb's folders lead. Lexical; nothing is read.
    pub(crate) fn file_tree_base(&self) -> Option<&Path> {
        self.file_tree
            .open
            .then(|| self.file_tree.ready_root())
            .flatten()
    }

    /// A key in the panel: the arrows move through the tree and open and
    /// close folders, Enter opens the row, and Escape leaves.
    fn file_tree_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
            return;
        }
        let in_search = self.file_tree.search.read(cx).focus.is_focused(window);
        if in_search && self.file_tree.search.read(cx).is_composing() {
            return;
        }
        let tree = &mut self.file_tree;
        let count = tree.rows.len();
        let selected = tree.selected_row();
        let handled = match keystroke.key.as_str() {
            "escape" => {
                if in_search && !tree.searching.query.is_empty() {
                    let search = tree.search.clone();
                    search.update(cx, |input, cx| input.set_text_at("", 0, cx));
                    self.search_files(String::new(), cx);
                } else {
                    window.focus(&self.focus, cx);
                }
                true
            }
            "down" if count > 0 => {
                let next = match selected {
                    Some(row) if !in_search => (row + 1).min(count - 1),
                    Some(row) => row,
                    None => 0,
                };
                tree.select(next);
                let focus = tree.focus.clone();
                window.focus(&focus, cx);
                true
            }
            "up" if count > 0 && !in_search => {
                match selected {
                    Some(0) | None => {
                        let focus = tree.search.read(cx).focus.clone();
                        window.focus(&focus, cx);
                    }
                    Some(row) => tree.select(row - 1),
                }
                true
            }
            "enter" => {
                if let Some(row) = selected.or((in_search && count > 0).then_some(0)) {
                    self.activate_file_row(row, window, cx);
                }
                true
            }
            key @ ("left" | "right") if !in_search => {
                if let Some(row) = selected {
                    self.file_tree_side_key(row, key == "right", cx);
                }
                true
            }
            _ => false,
        };
        if handled {
            cx.stop_propagation();
            window.prevent_default();
            cx.notify();
        }
    }

    /// Right opens a folder or steps into an open one; Left closes an open
    /// folder or goes to the folder holding the row.
    fn file_tree_side_key(&mut self, row: usize, right: bool, cx: &mut Context<Self>) {
        let Some(found) = self.file_tree.rows.get(row) else {
            return;
        };
        let path = found.path.clone();
        match (&found.what, right) {
            (rows::What::Folder { open: false }, true)
            | (rows::What::Folder { open: true }, false) => {
                self.toggle_folder(&path, cx);
            }
            (rows::What::Folder { open: true }, true) => self.file_tree.select(row + 1),
            (_, true) => {}
            (_, false) => {
                let depth = found.depth;
                let parent = self.file_tree.rows[..row]
                    .iter()
                    .rposition(|above| above.depth < depth);
                if let Some(parent) = parent {
                    self.file_tree.select(parent);
                }
            }
        }
    }

    /// Opens row `row`: a folder opens or closes, a file opens in its view.
    fn activate_file_row(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(found) = self.file_tree.rows.get(row) else {
            return;
        };
        let path = found.path.clone();
        self.file_tree.selected = Some(path.clone());
        match found.what {
            rows::What::Folder { .. } => self.toggle_folder(&path, cx),
            rows::What::File(_) => self.open_tree_file(path, window, cx),
            rows::What::Link => {
                self.show_flash(crate::window::Flash::warning(fl!("file-tree-link")), cx)
            }
            rows::What::Other
            | rows::What::More(_)
            | rows::What::Cut
            | rows::What::Loading
            | rows::What::Failed(_) => {}
        }
        cx.notify();
    }

    /// Sets the panel opening or closing to match the user's choice;
    /// whether it is moving, so the window draws again.
    pub(crate) fn update_file_tree(&mut self) -> bool {
        let tree = &mut self.file_tree;
        let now = motion::now();
        let first = !std::mem::replace(&mut tree.placed, true);
        let was = tree.motion.is_open();
        tree.motion.set(tree.open, now, first);
        was != tree.open || tree.motion.moving(now)
    }
}

#[cfg(test)]
mod tests;
