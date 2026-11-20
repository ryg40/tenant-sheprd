//! Drawing the file explorer: the search field and its controls, then the
//! prepared rows in a uniform list. Nothing here reads the disk.

use super::{
    Root,
    rows::{FileKind, Row, What},
};
use crate::{SheprdWindow, i18n::fl};
use gpui::{prelude::*, *};
use sheprd_client::text::safe_text;
use sheprd_vcs::file_tree::{self, Omitted};
use std::path::Path;

/// How far each level of the tree is set in.
const INDENT: f32 = 14.;

fn icon(what: &What) -> &'static str {
    match what {
        What::Folder { open: true } => "icons/chevron-down.svg",
        What::Folder { open: false } => "icons/chevron-right.svg",
        What::File(FileKind::Markdown) => "icons/note.svg",
        What::File(FileKind::Html) => "icons/globe.svg",
        What::Link => "icons/external.svg",
        What::More(_) | What::Cut => "icons/more.svg",
        What::Failed(_) => "icons/error.svg",
        What::File(FileKind::Other) | What::Other | What::Loading => "icons/file.svg",
    }
}

impl SheprdWindow {
    fn file_tree_button(
        &self,
        id: &'static str,
        icon: &'static str,
        label: String,
        cx: &mut Context<Self>,
        act: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let theme = &self.theme;
        div()
            .id(id)
            .debug_selector(move || id.into())
            .role(Role::Button)
            .aria_label(SharedString::from(label))
            .flex_none()
            .p(px(4.))
            .rounded(px(4.))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(theme.active)))
            .on_click(cx.listener(move |this, _, window, cx| act(this, window, cx)))
            .child(
                svg()
                    .path(icon)
                    .size(px(self.config.ui.size))
                    .text_color(rgb(theme.muted)),
            )
    }

    fn file_tree_row(
        &self,
        row: usize,
        active: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let tree = &self.file_tree;
        let found: &Row = tree.rows.get(row)?;
        let theme = &self.theme;
        let size = self.config.ui.size;
        let label: SharedString = match &found.what {
            What::More(Omitted::Exactly(count)) => fl!("file-tree-more", count = *count).into(),
            What::More(Omitted::AtLeast(count)) => {
                fl!("file-tree-more-at-least", count = *count).into()
            }
            What::Cut => fl!("file-tree-cut").into(),
            What::Loading => fl!("file-tree-loading").into(),
            What::Failed(error) => error.clone().into(),
            _ => found.label.clone(),
        };
        let entry = !matches!(
            found.what,
            What::More(_) | What::Cut | What::Loading | What::Failed(_)
        );
        let dim = found.ignored || !entry || matches!(found.what, What::Other | What::Link);
        let selected = tree.selected.as_deref() == Some(found.path.as_str());
        let shows = entry && active == Some(found.path.as_str());
        Some(
            div()
                .id(row)
                .debug_selector(move || format!("file-tree-row-{row}"))
                .w_full()
                .h(px(self.config.ui.line_height() + 6.))
                .pl(px(8. + INDENT * found.depth as f32))
                .pr(px(8.))
                .flex()
                .items_center()
                .gap(px(6.))
                .border_1()
                .border_color(if selected {
                    rgb(theme.primary())
                } else {
                    rgba(0)
                })
                .when(shows, |row| row.bg(rgb(theme.active)))
                .when(entry, |row| {
                    row.cursor_pointer()
                        .hover(|style| style.bg(rgb(theme.active)))
                })
                .text_color(rgb(if dim { theme.muted } else { theme.foreground }))
                .when(found.ignored, |row| row.opacity(0.6))
                .when(shows, |row| row.font_weight(FontWeight::SEMIBOLD))
                .child(
                    svg()
                        .path(icon(&found.what))
                        .size(px(size))
                        .flex_none()
                        .text_color(rgb(if shows { theme.primary() } else { theme.muted })),
                )
                .child(div().flex_1().min_w_0().truncate().child(label))
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&this.file_tree.focus, cx);
                    this.activate_file_row(row, window, cx);
                }))
                .into_any_element(),
        )
    }

    /// The row above the tree that names its root: the checkout, or the
    /// pane's folder outside one. The whole path shows on hover. The folder
    /// comes from the daemon's pane, so its names are untrusted: they are
    /// bounded and lose the characters the tree lists no name with.
    fn file_tree_root_row(&self, root: &Path) -> Stateful<Div> {
        let theme = &self.theme;
        let full = root.to_string_lossy();
        let path: SharedString = safe_text(&full, file_tree::MAX_PATH_BYTES).into();
        let name: SharedString = root.file_name().map_or_else(
            || path.clone(),
            |name| safe_text(&name.to_string_lossy(), file_tree::MAX_NAME_BYTES).into(),
        );
        let (foreground, surface) = (theme.foreground, theme.sidebar_background());
        div()
            .id("file-tree-root")
            .debug_selector(|| "file-tree-root".into())
            .flex_none()
            .px(px(8.))
            .pt(px(6.))
            .pb(px(2.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(theme.muted))
            .truncate()
            .child(name)
            .tooltip(move |_, cx| {
                cx.new(|_| crate::usage::Hint {
                    text: path.clone(),
                    foreground,
                    surface,
                })
                .into()
            })
    }

    pub(crate) fn render_file_tree(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = &self.theme;
        let tree = &self.file_tree;
        let size = self.config.ui.size;
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .px(px(8.))
            .py(px(6.))
            .border_b_1()
            .border_color(rgb(theme.active))
            .child(self.file_tree_button(
                "file-tree-refresh",
                "icons/refresh.svg",
                fl!("file-tree-refresh"),
                cx,
                |this, _, cx| this.refresh_file_tree(cx),
            ))
            .child(
                div()
                    .debug_selector(|| "file-tree-search".into())
                    .flex_1()
                    .min_w_0()
                    .child(tree.search.clone()),
            )
            .child(self.file_tree_button(
                "file-tree-collapse",
                "icons/chevron-up.svg",
                fl!("file-tree-collapse"),
                cx,
                |this, _, cx| this.collapse_file_tree(cx),
            ));
        let searching = !tree.searching.query.is_empty();
        let state = match &tree.root {
            Root::None => Some(fl!("file-tree-no-folder")),
            Root::Locating => Some(fl!("file-tree-loading")),
            Root::Remote => Some(fl!("file-tree-remote")),
            Root::Failed(error) => Some(error.clone()),
            Root::Ready(_) if searching => match (&tree.searching.failed, &tree.searching.hits) {
                (Some(error), _) => Some(error.clone()),
                (None, None) => Some(fl!("file-tree-searching")),
                (None, Some(hits)) if hits.is_empty() => Some(fl!("file-tree-no-match")),
                _ => None,
            },
            Root::Ready(_) => None,
        };
        let body = match state {
            Some(text) => div()
                .debug_selector(|| "file-tree-state".into())
                .flex_1()
                .p(px(12.))
                .text_color(rgb(theme.muted))
                .child(text)
                .into_any_element(),
            None => uniform_list(
                "file-tree-rows",
                tree.rows.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    let active = this.file_tree_active(cx);
                    range
                        .filter_map(|row| this.file_tree_row(row, active.as_deref(), cx))
                        .collect()
                }),
            )
            .track_scroll(&tree.scroll)
            .flex_1()
            .min_h_0()
            .into_any_element(),
        };
        div()
            .id("file-tree")
            .debug_selector(|| "file-tree".into())
            .role(Role::Complementary)
            .aria_label(SharedString::from(fl!("file-tree-title")))
            .track_focus(&tree.focus)
            .on_key_down(cx.listener(Self::file_tree_key))
            .flex()
            .flex_col()
            .h_full()
            .min_h_0()
            .bg(self.fill().panel(theme.sidebar_background()))
            .border_l_1()
            .border_color(rgb(theme.active))
            .text_size(px(size))
            .text_color(rgb(theme.foreground))
            .child(header)
            .when_some(tree.ready_root(), |panel, root| {
                panel.child(self.file_tree_root_row(root))
            })
            .child(body)
    }
}
