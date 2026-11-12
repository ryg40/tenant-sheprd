//! Opening a file chosen in the tree, in the view every other way of
//! opening a file gives it (`browser::file_view::view_for`): a page for a
//! Markdown or HTML file, its source where it has no page, a code tab for
//! any other file.

use crate::{
    SheprdWindow,
    browser::{
        LocalFile, Location, Store,
        file_view::{FileView, view_for},
    },
    editor::EditorTarget,
    i18n::{Localize, fl},
    window::Flash,
};
use gpui::{Context, Window};
use sheprd_vcs::file_tree;
use std::path::{Path, PathBuf};

impl SheprdWindow {
    /// Opens the file at `path`, relative to the root, once a background
    /// check finds it a regular file inside the root.
    pub(super) fn open_tree_file(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_tree_file_as(path, crate::browser::EMBEDDED, window, cx);
    }

    /// [`Self::open_tree_file`], on a build that shows pages inside the
    /// window or not (`embedded`). The view is the one [`view_for`] gives
    /// every way of opening a file: a page, the source of a Markdown or
    /// HTML file that has no page here, whose bar says why, or code.
    pub(super) fn open_tree_file_as(
        &mut self,
        path: String,
        embedded: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.file_tree.ready_root().map(Path::to_owned) else {
            return;
        };
        let serial = self.file_tree.serial;
        let checked = cx.background_executor().spawn(async move {
            let file = file_tree::file(&root, &path)?;
            let home = crate::config::home().ok();
            let view = view_for(&file, home.as_deref(), embedded)?;
            Ok::<_, crate::Error>((file, view))
        });
        cx.spawn_in(window, async move |this, cx| {
            let checked = checked.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.file_tree.serial != serial {
                    return;
                }
                match checked {
                    Ok((file, FileView::Page(page))) => this.open_tree_page(file, page, window, cx),
                    Ok((file, FileView::Source(..) | FileView::Code(_))) => {
                        this.open_code_view(
                            &EditorTarget {
                                path: file,
                                line: None,
                            },
                            window,
                            cx,
                        );
                    }
                    Err(error) => this.show_flash(Flash::warning(error.localize()), cx),
                }
            });
        })
        .detach();
    }

    /// Shows `page` in a browser tab of the focused workspace, the one
    /// already on that file when there is one.
    fn open_tree_page(
        &mut self,
        file: PathBuf,
        page: LocalFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((scope, workspace)) = self.browser_key() else {
            self.show_flash(Flash::warning(fl!("browser-open-workspace-first")), cx);
            return;
        };
        let location = Location::Local { file: page };
        let before = cx
            .try_global::<Store>()
            .and_then(|store| store.opened_before(&scope, &workspace, None, &location));
        let opened = before.or_else(|| {
            Store::update(cx, |store| {
                store.open(scope, &workspace, Some(location), None)
            })
        });
        match opened {
            Some(id) => self.show_browser_tab(id, window, cx),
            // Too many page tabs: the file still opens, as its text.
            None => self.open_code_view(
                &EditorTarget {
                    path: file,
                    line: None,
                },
                window,
                cx,
            ),
        }
    }

    /// The file the active tab shows, relative to the tree's root, which
    /// the tree marks.
    pub(super) fn file_tree_active(&self, cx: &gpui::App) -> Option<String> {
        let root = self.file_tree.ready_root()?;
        let crate::browser::Shown::Page(id) = self.group_shown(self.active_group()?, cx) else {
            return None;
        };
        let path = match cx.try_global::<Store>()?.get(id)?.location.as_ref()? {
            Location::Code { file } => PathBuf::from(&file.path),
            Location::Local { file } => file.path(),
            _ => return None,
        };
        file_tree::locate(root, &path).ok()
    }
}
