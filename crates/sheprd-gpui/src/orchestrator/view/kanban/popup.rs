//! Read-only card detail and its reversible modal lifetime.
use super::*;
use crate::{config::corners, motion};

#[derive(Default)]
pub(super) struct Popup {
    pub(super) card: Option<rows::Card>,
    focus: Option<FocusHandle>,
    focus_pending: bool,
    motion: motion::Toggle,
}

impl Popup {
    pub(super) fn open(&mut self, card: rows::Card, cx: &mut Context<OrchestratorView>) {
        self.card = Some(card);
        self.focus.get_or_insert_with(|| cx.focus_handle());
        self.focus_pending = !self.motion.is_open();
        self.motion.set(true, motion::now(), false);
    }

    pub(super) fn close(&mut self) {
        self.focus_pending = false;
        self.motion.set(false, motion::now(), false);
    }

    pub(super) fn is_shown(&self) -> bool {
        self.card.is_some() && (self.motion.is_open() || self.motion.moving(motion::now()))
    }
}

impl OrchestratorView {
    pub(in crate::orchestrator::view) fn kanban_detail_item(
        &self,
        key: &str,
    ) -> Option<&sheprd_orchestrator::Item> {
        self.kanban
            .popup
            .card
            .as_ref()
            .filter(|card| card.summary.key == key)
            .map(|card| &card.domain.item)
    }

    pub(in crate::orchestrator::view) fn close_kanban_popup(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.detail = None;
        self.kanban.popup.close();
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(super) fn focus_kanban_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = &self.kanban.popup.focus {
            window.focus(focus, cx);
            self.kanban.popup.focus_pending = false;
        }
    }

    pub(super) fn kanban_popup_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Consume every key, including search and board navigation, while modal.
        cx.stop_propagation();
        let modifiers = &event.keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt {
            return;
        }
        match event.keystroke.key.as_str() {
            "escape" => self.close_kanban_popup(window, cx),
            "up" | "k" if self.detail.is_some() => self.step_kanban_detail(-1, cx),
            "down" | "j" if self.detail.is_some() => self.step_kanban_detail(1, cx),
            // Reopening during leave reverses from the current opacity.
            "enter" if self.detail.is_none() => {
                if let Some(key) = self
                    .kanban
                    .popup
                    .card
                    .as_ref()
                    .map(|card| card.summary.key.clone())
                {
                    self.open_kanban_key(&key, cx);
                    self.focus_kanban_popup(window, cx);
                }
            }
            _ => {}
        }
    }

    pub(super) fn render_kanban_popup(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let now = motion::now();
        let popup = &mut self.kanban.popup;
        if !popup.motion.is_open() {
            if popup
                .focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window))
            {
                window.focus(&self.focus, cx);
            }
            if !popup.motion.moving(now) {
                popup.card = None;
                return None;
            }
        }
        let card = popup.card.as_ref()?;
        let detail = Detail {
            key: card.summary.key.clone(),
            tab: DetailTab::Description,
        };
        let name = card.name.clone();
        if popup.motion.moving(now) {
            window.request_animation_frame();
        }
        let shown = popup.motion.shown(now);
        if popup.focus_pending {
            self.focus_kanban_popup(window, cx);
        }
        let focus = self.kanban.popup.focus.clone()?;
        let pane = self.render_detail(detail, window, cx);
        let theme = &self.look.theme;
        Some(
            div()
                .id("kanban-popup-backdrop")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(0x00000080))
                .opacity(shown)
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        this.close_kanban_popup(window, cx);
                    }),
                )
                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .on_scroll(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .id("kanban-popup")
                        .role(Role::Dialog)
                        .aria_label(name)
                        .track_focus(&focus)
                        .key_context("KanbanDetail")
                        .w(relative(0.8))
                        .h(relative(0.8))
                        .min_w_0()
                        .min_h_0()
                        .mt(px((1. - shown) * 12.))
                        .flex()
                        .flex_col()
                        .rounded(px(corners::PANEL))
                        .border_1()
                        .border_color(rgb(theme.active))
                        .bg(rgb(theme.background))
                        .shadow_lg()
                        .overflow_hidden()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_key_down(cx.listener(Self::kanban_popup_key))
                        .child(pane),
                )
                .into_any_element(),
        )
    }

    pub(in crate::orchestrator::view) fn kanban_detail_metadata(&self) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .children(self.kanban.popup.card.as_ref().map(|card| {
                div()
                    .child(card.warnings.clone())
                    .child(card.mapping.clone())
            }))
    }
}
