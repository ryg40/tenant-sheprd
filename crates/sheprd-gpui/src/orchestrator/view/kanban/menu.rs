//! Keyboard column choices propose the same native dialog as a card drop.
use super::*;
use crate::motion;
use sheprd_control_protocol::BoardColumn;

pub(super) struct Menu {
    pub(super) card: CardKey,
    pub(super) column: usize,
    pub(super) focus: FocusHandle,
    motion: motion::Toggle,
    pub(super) revision: u64,
}

impl State {
    pub(super) fn move_menu_open(&self) -> bool {
        self.move_menu
            .as_ref()
            .is_some_and(|menu| menu.motion.is_open())
    }
}

impl OrchestratorView {
    pub(super) fn open_move_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(card) = self.kanban.selected.clone() else {
            return;
        };
        if self.kanban.move_dialog.is_some() {
            return;
        }
        if let Some(menu) = &mut self.kanban.move_menu {
            menu.card = card;
            menu.revision = self.kanban.revision;
            menu.motion.set(true, motion::now(), false);
            window.focus(&menu.focus, cx);
        } else {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            let mut motion = motion::Toggle::default();
            motion.set(true, motion::now(), false);
            self.kanban.move_menu = Some(Menu {
                card,
                column: 0,
                focus,
                motion,
                revision: self.kanban.revision,
            });
        }
        cx.notify();
    }

    pub(super) fn move_menu_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        let Some(menu) = self
            .kanban
            .move_menu
            .as_mut()
            .filter(|menu| menu.motion.is_open())
        else {
            return;
        };
        match event.keystroke.key.as_str() {
            "escape" => self.close_move_menu(window, cx),
            "up" | "left" => menu.column = menu.column.saturating_sub(1),
            "down" | "right" | "tab" => menu.column = (menu.column + 1) % BoardColumn::ALL.len(),
            "enter" | "space" => {
                let (card, column, revision) = (
                    menu.card.clone(),
                    BoardColumn::ALL[menu.column],
                    menu.revision,
                );
                self.choose_move_column(card, column, revision, window, cx);
            }
            _ => {}
        }
        cx.notify();
    }

    fn close_move_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = &mut self.kanban.move_menu {
            menu.motion.set(false, motion::now(), false);
        }
        self.restore_move_focus(window, cx);
    }

    fn choose_move_column(
        &mut self,
        card: CardKey,
        column: BoardColumn,
        revision: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.kanban.move_menu_open() {
            return;
        }
        self.close_move_menu(window, cx);
        let result = if revision != self.kanban.revision {
            Err(error::Error::Model(
                sheprd_orchestrator::kanban::Error::StaleMove,
            ))
        } else {
            self.propose_card(&card.item.canonical(), &card.lane, column, window, cx)
        };
        if let Err(error) = result {
            self.kanban.error = Some(error);
            self.restore_move_focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn render_move_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.kanban.move_menu.as_ref()?;
        let now = motion::now();
        if !menu.motion.is_open() && !menu.motion.moving(now) {
            self.kanban.move_menu = None;
            return None;
        }
        if menu.motion.moving(now) {
            window.request_animation_frame();
        }
        let shown = menu.motion.shown(now);
        let open = menu.motion.is_open();
        let theme = &self.look.theme;
        let mut choices = div().flex().flex_col().gap_2();
        for (index, column) in BoardColumn::ALL.iter().copied().enumerate() {
            let card = menu.card.clone();
            let revision = menu.revision;
            choices = choices.child(
                self.look
                    .button(
                        ("move-choice", index),
                        rows::column(column),
                        index == menu.column,
                    )
                    .when(open, |el| {
                        el.on_click(cx.listener(move |this, _, window, cx| {
                            this.choose_move_column(card.clone(), column, revision, window, cx)
                        }))
                    }),
            );
        }
        Some(
            div()
                .absolute()
                .inset_0()
                .when(open, |el| {
                    el.occlude()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                        .on_scroll(|_, _, cx| cx.stop_propagation())
                })
                .bg(rgba(0x00000080))
                .opacity(shown)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .id("kanban-move-menu")
                        .role(Role::Dialog)
                        .aria_label(fl!("kanban-move-menu"))
                        .when(open, |el| {
                            el.track_focus(&menu.focus)
                                .key_context("KanbanMoveMenu")
                                .on_key_down(cx.listener(Self::move_menu_key))
                        })
                        .mt(px((1. - shown) * 12.))
                        .p_4()
                        .max_w_full()
                        .max_h_full()
                        .overflow_y_scroll()
                        .bg(rgb(theme.background))
                        .child(fl!("kanban-move-menu"))
                        .child(choices),
                )
                .into_any_element(),
        )
    }
}
