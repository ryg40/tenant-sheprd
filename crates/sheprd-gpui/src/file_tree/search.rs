//! The explorer's search: files by name, ranked by Go to File's index, in
//! place of the tree. Typing waits [`DELAY`] before anything is ranked, one
//! index is read for a root at a time, and only the answer to the query
//! and root that still show is taken.

use crate::{
    SheprdWindow,
    code_index::Index,
    code_search::{Mode, Query, rank},
    i18n::Localize,
};
use gpui::{prelude::*, *};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

/// How long the field rests before its text is searched for, so a word
/// typed quickly is one search.
pub(super) const DELAY: Duration = Duration::from_millis(150);

/// What the search field lists in place of the tree.
#[derive(Debug, Default)]
pub(super) struct Search {
    /// The index of this root being read; no second read starts beside it.
    building: Option<Task<()>>,
    /// How many index reads started, for tests.
    #[cfg(test)]
    pub(super) builds: usize,
    pub(super) failed: Option<String>,
    /// The files found, relative to the root; `None` while the first
    /// ranking of this search runs.
    pub(super) hits: Option<Vec<String>>,
    pub(super) query: String,
    /// A refresh asked for a newer index while one was being read.
    stale: bool,
    /// The wait after typing, then the ranking of the query.
    task: Option<Task<()>>,
}

impl Search {
    /// Forgets what was found, and drops the work for a root left behind.
    /// The query stays: it is searched for again under the next root.
    pub(super) fn reset(&mut self) {
        self.building = None;
        self.failed = None;
        self.hits = None;
        self.stale = false;
        self.task = None;
    }
}

impl SheprdWindow {
    /// Lists the files whose name matches `query` in place of the tree,
    /// once typing rests; an empty query shows the tree again at once.
    pub(super) fn search_files(&mut self, query: String, cx: &mut Context<Self>) {
        let tree = &mut self.file_tree;
        tree.searching.failed = None;
        if query.is_empty() {
            tree.searching.query = query;
            tree.searching.hits = None;
            tree.searching.task = None;
            tree.rebuild();
            cx.notify();
            return;
        }
        tree.searching.query = query;
        // Replacing the task drops the wait for the text before this one.
        tree.searching.task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DELAY).await;
            let _ = this.update(cx, |this, cx| this.rank_files(false, cx));
        }));
        cx.notify();
    }

    /// Ranks the query against the root's Go to File index. The index is
    /// read first when the window keeps none, or when `fresh`.
    pub(super) fn rank_files(&mut self, fresh: bool, cx: &mut Context<Self>) {
        let tree = &self.file_tree;
        let Some(root) = tree.ready_root().map(Path::to_owned) else {
            return;
        };
        let query = tree.searching.query.clone();
        if query.is_empty() {
            return;
        }
        let kept = self.code_indexes.get(&root).map(|(index, _)| index.clone());
        let Some(index) = kept.filter(|_| !fresh) else {
            return self.build_file_index(root, fresh, cx);
        };
        let serial = self.file_tree.serial;
        let asked = query.clone();
        let ranked = cx.background_executor().spawn(async move {
            rank(&index, Mode::Files, &Query::parse(&query, Mode::Files))
                .into_iter()
                .filter_map(|hit| index.files().get(hit.item).cloned())
                .collect::<Vec<String>>()
        });
        self.file_tree.searching.task = Some(cx.spawn(async move |this, cx| {
            let hits = ranked.await;
            let _ = this.update(cx, |this, cx| {
                let tree = &mut this.file_tree;
                // The field or the root moved on: a later ranking answers.
                if tree.serial != serial || tree.searching.query != asked {
                    return;
                }
                tree.searching.hits = Some(hits);
                tree.searching.failed = None;
                tree.selected = None;
                tree.rebuild();
                tree.scroll.scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
            });
        }));
    }

    /// Reads the index of `root`, then ranks the query the field holds by
    /// then. While a read runs no other starts: a `fresh` one is noted and
    /// follows it.
    fn build_file_index(&mut self, root: PathBuf, fresh: bool, cx: &mut Context<Self>) {
        let tree = &mut self.file_tree;
        if tree.searching.building.is_some() {
            tree.searching.stale |= fresh;
            return;
        }
        #[cfg(test)]
        {
            tree.searching.builds += 1;
        }
        let (cancelled, serial) = (tree.cancelled.clone(), tree.serial);
        let built = cx
            .background_executor()
            .spawn(async move { Index::build(&root, &cancelled).map(Arc::new) });
        tree.searching.building = Some(cx.spawn(async move |this, cx| {
            let built = built.await;
            let _ = this.update(cx, |this, cx| {
                // Another root dropped this task, and reads its own index.
                if this.file_tree.serial != serial {
                    return;
                }
                this.file_tree.searching.building = None;
                let again = std::mem::take(&mut this.file_tree.searching.stale);
                match built {
                    Ok(index) => {
                        this.code_indexes.insert(index, Instant::now());
                        this.rank_files(again, cx);
                    }
                    Err(error) => {
                        let tree = &mut this.file_tree;
                        tree.searching.failed = Some(crate::Error::from(error).localize());
                        tree.searching.hits = Some(Vec::new());
                        tree.rebuild();
                        cx.notify();
                    }
                }
            });
        }));
    }
}
