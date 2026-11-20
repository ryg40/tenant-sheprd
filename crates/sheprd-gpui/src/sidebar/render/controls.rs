//! The sidebar's two controls for the whole list: a new workspace, and the
//! app's menu. Most layouts draw them as the terminal client's lowercase
//! links under the list; the desktop chrome
//! ([`crate::config::LayoutMode::desktop_chrome`]) draws them as icon
//! buttons at the end of the list's heading instead.

use crate::{
    Command, SheprdWindow, chrome_button,
    config::FontConfig,
    direction::Logical,
    i18n::fl,
    sidebar::{label_text, layout::SidebarLook, line_height, rail::Hint},
};
use gpui::{prelude::*, *};

/// The space between the heading's icon buttons.
const BUTTON_GAP: f32 = 2.;

impl SheprdWindow {
    /// The links under the list: `new` at the start, and `menu` at the
    /// sidebar's end.
    pub(super) fn sidebar_links(
        &self,
        font: &FontConfig,
        look: SidebarLook,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = &self.theme;
        div()
            .flex_none()
            .h(px(line_height(font) + 2. * look.density.footer_padding()))
            .px(px(look.content_x()))
            .row()
            .items_center()
            // Menu hugs the sidebar's edge, as in the terminal client.
            .justify_between()
            .text_color(rgb(theme.muted))
            .gap(px(20.))
            .child(
                div()
                    .id("new-workspace")
                    .debug_selector(|| "new-workspace".into())
                    .map(|button| self.sidebar_button(button, fl!("sidebar-new-workspace")))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme.foreground)))
                    .child(fl!("sidebar-new"))
                    .on_click(cx.listener(new_workspace)),
            )
            .child(
                div()
                    .id("sidebar-menu")
                    .debug_selector(|| "sidebar-menu".into())
                    .map(|button| self.sidebar_button(button, fl!("sidebar-menu-label")))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme.foreground)))
                    .child(label_text(&fl!("sidebar-menu")))
                    .on_click(cx.listener(open_menu)),
            )
    }

    /// `heading` with the two controls at its end as icon buttons. The
    /// heading grows to hold them, should a small face or tight spacing
    /// leave it shorter.
    pub(super) fn with_heading_controls(&self, heading: Div, cx: &mut Context<Self>) -> Div {
        heading
            .row()
            .min_h(px(chrome_button::SIZE))
            .justify_between()
            .gap(px(BUTTON_GAP))
            .child(
                div()
                    .flex_none()
                    .row()
                    .items_center()
                    .gap(px(BUTTON_GAP))
                    .child(self.new_workspace_button(cx))
                    .child(self.menu_button(cx)),
            )
    }

    /// The heading's `+`, which creates a workspace.
    pub(in crate::sidebar) fn new_workspace_button(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        self.heading_button(
            "new-workspace",
            "icons/plus.svg",
            fl!("sidebar-new-workspace"),
        )
        .on_click(cx.listener(new_workspace))
    }

    /// The heading's `...`, which opens the menu.
    pub(in crate::sidebar) fn menu_button(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        self.heading_button("sidebar-menu", "icons/more.svg", fl!("sidebar-menu-label"))
            .on_click(cx.listener(open_menu))
    }

    /// An icon button named `name`, which its tooltip says too, as one of
    /// the sidebar's stops.
    fn heading_button(&self, id: &'static str, icon: &'static str, name: String) -> Stateful<Div> {
        let button = chrome_button::icon_button(id, icon, chrome_button::RADIUS, &self.theme)
            .debug_selector(move || id.into())
            .tooltip(Hint::new(&self.theme).with(name.clone()));
        self.sidebar_button(button, name)
    }
}

fn new_workspace(
    this: &mut SheprdWindow,
    _: &ClickEvent,
    window: &mut Window,
    cx: &mut Context<SheprdWindow>,
) {
    this.command(Command::Workspace, window, cx);
}

/// Opens the menu where the click landed; a key's click lands on the
/// control's corner.
fn open_menu(
    this: &mut SheprdWindow,
    event: &ClickEvent,
    window: &mut Window,
    cx: &mut Context<SheprdWindow>,
) {
    this.menu.anchor = event.position();
    this.open_menu(window, cx);
}
