//! Drawing a file view's bar: the breadcrumb, the switch, and the editor.

use super::{FileView, crumb_folder, crumbs, renders, source_only, tab_file, view_for};
use crate::{
    SheprdWindow,
    browser::{Location, Tab, groups::Slot},
    editor::EditorTarget,
    i18n::fl,
};
use gpui::{prelude::*, *};
use std::path::Path;

/// How much sooner a breadcrumb's folders give way than its file's name.
const FOLDERS_SHRINK: f32 = 100.;

impl SheprdWindow {
    /// One half of the switch, or the editor's button.
    fn file_bar_button(
        &self,
        slot: Slot,
        id: &'static str,
        label: String,
        chosen: bool,
    ) -> Stateful<Div> {
        let theme = &self.theme;
        let wash = theme.primary_wash();
        div()
            .id(id)
            .debug_selector(move || slot.selector(id))
            .flex_none()
            .px(px(8.))
            .h(px(20.))
            .flex()
            .items_center()
            .rounded(px(crate::config::corners::CONTROL))
            .cursor_pointer()
            .map(|button| {
                if chosen {
                    button.bg(rgb(wash)).text_color(rgb(theme.text_on(wash)))
                } else {
                    button
                        .text_color(rgb(theme.foreground))
                        .hover(|button| button.bg(rgb(theme.active)))
                }
            })
            .child(label)
    }

    /// The bar above a local file: where it is, and for a Markdown or HTML
    /// file the switch between its page and its source. `None` for a tab
    /// that shows no local file.
    pub(crate) fn file_bar(&self, slot: Slot, tab: &Tab, cx: &mut Context<Self>) -> Option<Div> {
        let path = tab_file(tab)?;
        let id = tab.id;
        let theme = &self.theme;
        let local = matches!(tab.location, Some(Location::Local { .. }));
        let source = !local || self.browser.file_views.shows_source(tab);
        let home = crate::config::home().ok();
        let root = self.local_editor_root().map(Path::new);
        let names = crumbs(&path, root, home.as_deref());
        let last = names.len().saturating_sub(1);
        // A folder the file explorer lists, or will list once it shows,
        // leads there. The click's own test, which reads nothing from disk,
        // so no name is a link that the click then refuses.
        let listed = self.file_tree_base().or(root);
        let folders: Vec<Option<std::path::PathBuf>> = (0..names.len())
            .map(|index| {
                crumb_folder(&path, &names, index)
                    .filter(|folder| {
                        listed.is_some_and(|listed| {
                            sheprd_vcs::file_tree::locate(listed, folder).is_ok()
                        })
                    })
                    .map(Path::to_owned)
            })
            .collect();
        let mut trail = Vec::new();
        for (index, (name, folder)) in names.into_iter().zip(folders).enumerate() {
            let chevron = (index > 0).then(|| {
                svg()
                    .path(crate::direction::icon("icons/chevron-right.svg"))
                    .size(px(10.))
                    .flex_none()
                    .text_color(rgb(theme.muted))
                    .into_any_element()
            });
            let color = if index == last {
                theme.foreground
            } else {
                theme.muted
            };
            // A narrow bar shortens the folders and keeps the file's name.
            let name = div()
                .min_w_0()
                .truncate()
                .when(index != last, |name| name.flex_shrink(FOLDERS_SHRINK))
                .text_color(rgb(color))
                .child(name);
            let name = match folder {
                Some(folder) => name
                    .id(ElementId::Name(
                        slot.selector(&format!("file-crumb-{index}")).into(),
                    ))
                    .debug_selector(move || slot.selector(&format!("file-crumb-{index}")))
                    .cursor_pointer()
                    .hover(|name| name.text_color(rgb(theme.foreground)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.show_folder_in_file_tree(folder.clone(), window, cx);
                    }))
                    .into_any_element(),
                None => name.into_any_element(),
            };
            trail.extend(chevron);
            trail.push(name);
        }
        let switch = renders(&path).then(|| {
            div()
                .debug_selector(move || slot.selector("file-switch"))
                .flex_none()
                .flex()
                .gap(px(2.))
                .child(
                    self.file_bar_button(
                        slot,
                        "file-rendered",
                        fl!("browser-file-rendered"),
                        !source,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.show_file_source(id, false, true, window, cx);
                    })),
                )
                .child(
                    self.file_bar_button(slot, "file-source", fl!("browser-file-source"), source)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.show_file_source(id, true, true, window, cx);
                        })),
                )
        });
        // A Markdown or HTML file that never renders here says why beside
        // its switch, so the reason is there before "Rendered" is tried.
        let why = (!local && renders(&path))
            .then(|| view_for(&path, home.as_deref(), crate::browser::EMBEDDED).ok())
            .flatten()
            .and_then(|view| match view {
                FileView::Source(_, why) => Some(why),
                FileView::Page(_) | FileView::Code(_) => None,
            })
            .map(|why| {
                div()
                    .debug_selector(move || slot.selector("file-source-only"))
                    .min_w_0()
                    .truncate()
                    .text_color(rgb(theme.muted))
                    .child(source_only(why))
            });
        // The code view has its own way to the editor, at the line in view.
        // The terminal editor takes text: a page file, not an image.
        let editor = (!source && renders(&path)).then(|| {
            let target = EditorTarget {
                path: path.clone(),
                line: None,
            };
            self.file_bar_button(
                slot,
                "file-open-editor",
                fl!("code-view-open-in-editor"),
                false,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_in_editor(&target, None, cx);
            }))
        });
        Some(
            div()
                .debug_selector(move || slot.selector("file-bar"))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(10.))
                .py(px(3.))
                .bg(rgb(theme.surface))
                .border_b_1()
                .border_color(rgb(theme.active))
                .child(
                    div()
                        .debug_selector(move || slot.selector("file-crumbs"))
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .overflow_hidden()
                        .children(trail),
                )
                .children(why)
                .children(switch)
                .children(editor),
        )
    }

    /// A code tab. One of a Markdown or HTML file has the bar above it, for
    /// the way to the file's page; any other names its file itself.
    pub(crate) fn render_code_file_tab(
        &self,
        slot: Slot,
        tab: &Tab,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let code = self.render_code_tab(slot, tab, cx);
        let bar = tab_file(tab)
            .filter(|path| renders(path))
            .and_then(|_| self.file_bar(slot, tab, cx));
        match bar {
            Some(bar) => div()
                .size_full()
                .flex()
                .flex_col()
                .child(bar)
                .child(div().flex_1().min_h_0().child(code))
                .into_any_element(),
            None => code,
        }
    }
}
