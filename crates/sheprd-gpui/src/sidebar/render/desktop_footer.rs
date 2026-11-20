//! The device footer of the desktop chrome
//! ([`crate::config::LayoutMode::desktop_chrome`]): rows of an icon and a
//! label, as a desktop app's sidebar ends. The devices row opens the device
//! picker, and a Settings row opens the settings window, unless the
//! activity bar, which has a Settings button of its own, is shown.

use crate::{
    Command, SheprdWindow,
    config::corners,
    direction::Logical,
    i18n::fl,
    sidebar::{DEVICE_FOOTER_HEIGHT, label_text, line_height, rail::Hint},
};
use gpui::{prelude::*, *};
use std::{cell::Cell, rc::Rc};

/// A row's least height: its height at the default size.
const ROW: f32 = 30.;

/// The least room above and below a row's label, so a large `[sidebar]
/// size` makes the rows taller rather than running the label out of them.
const LABEL_ROOM: f32 = 6.;

/// The space around the rows and between them.
const PADDING: f32 = 4.;

/// A row's icon.
const ICON: f32 = 16.;

impl SheprdWindow {
    /// The device footer's height as drawn: none while it is hidden, its
    /// rows in the desktop chrome, and the one bar otherwise.
    pub(in crate::sidebar) fn device_footer_height(&self) -> f32 {
        if !self.device_footer_shown() {
            return 0.;
        }
        if !self.config.layout.mode.desktop_chrome() {
            return DEVICE_FOOTER_HEIGHT;
        }
        let rows = if self.activity_bar_shown() { 1. } else { 2. };
        // The rows, the padding around and between them, and the top border.
        rows * self.footer_row_height() + (rows + 1.) * PADDING + 1.
    }

    /// A row's height: [`ROW`], or the label's line with room around it
    /// when `[sidebar] size` makes that taller. Whole pixels keep the
    /// edges crisp.
    fn footer_row_height(&self) -> f32 {
        (line_height(&self.config.sidebar_font()) + 2. * LABEL_ROOM)
            .ceil()
            .max(ROW)
    }

    /// The footer, as tall as its rows make it, which
    /// [`Self::device_footer_height`] works out the same way for the list
    /// above it.
    pub(in crate::sidebar) fn render_desktop_footer(
        &self,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = &self.theme;
        let row = self.footer_row_height();
        let bounds = Rc::new(Cell::new(Bounds::<Pixels>::default()));
        let anchor = bounds.clone();
        let (label, connected) = self.device_scope();
        let hosts = self.endpoints.len();
        let picker = self
            .footer_row(
                "device-picker",
                row,
                if connected {
                    fl!("devices-picker-connected", device = label.as_str())
                } else {
                    fl!("devices-picker-disconnected", device = label.as_str())
                },
            )
            .relative()
            .child(
                div()
                    .debug_selector(|| "device-footer-icon".into())
                    .relative()
                    .flex_none()
                    .child(crate::icons::icon(
                        "icons/devices.svg",
                        ICON,
                        theme.foreground,
                    ))
                    .children(self.device_adding_badge()),
            )
            .child(
                div()
                    .debug_selector(|| "device-footer-label".into())
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(label_text(&label)),
            )
            .when(hosts > 1, |row| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(rgb(theme.muted))
                        .child(fl!("devices-hosts", count = hosts)),
                )
            })
            .child(self.device_status_dot(connected))
            .child(self.device_picker_probe(bounds))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_device_picker(anchor.get().origin, window, cx);
            }));
        div()
            .id("device-footer")
            .debug_selector(|| "device-footer".into())
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(PADDING))
            .p(px(PADDING))
            .border_t_1()
            .border_color(rgb(theme.active))
            .child(picker)
            .when(!self.activity_bar_shown(), |footer| {
                let name = fl!("devices-settings");
                footer.child(
                    self.footer_row("device-settings", row, name.clone())
                        .tooltip(Hint::new(theme).with(self.settings_hint()))
                        .child(crate::icons::icon(
                            "icons/settings.svg",
                            ICON,
                            theme.foreground,
                        ))
                        .child(div().flex_1().min_w_0().truncate().child(label_text(&name)))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.command(Command::Settings, window, cx);
                        })),
                )
            })
    }

    /// A footer row `height` px tall named `name`: one of the sidebar's
    /// stops, filled on hover.
    fn footer_row(&self, id: &'static str, height: f32, name: String) -> Stateful<Div> {
        let hover = self.theme.active;
        let row = div().id(id).debug_selector(move || id.into());
        self.sidebar_button(row, name)
            .flex_none()
            .h(px(height))
            .row()
            .items_center()
            .gap(px(8.))
            .px(px(8.))
            .rounded(px(corners::CONTROL))
            .cursor_pointer()
            .hover(move |style| style.bg(rgb(hover)))
    }
}
