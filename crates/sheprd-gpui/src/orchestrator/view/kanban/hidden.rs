//! Hiding a lane's repository retires what acts on its cards: the selected
//! card, its popup, and its move menu close, and a move not yet confirmed
//! is withdrawn. A move already submitted keeps running.
use super::*;

impl OrchestratorView {
    pub(in crate::orchestrator::view) fn retire_hidden_cards(&mut self) {
        let select = &self.repo_select;
        let hidden = |card: &CardKey| !select.shows(Some(&card.lane.canonical()));
        if self.kanban.selected.as_ref().is_some_and(hidden) {
            self.kanban.selected = None;
        }
        if self
            .kanban
            .popup
            .card
            .as_ref()
            .is_some_and(|card| hidden(&card.domain.key))
        {
            self.kanban.popup.close();
            if self.kanban.shown {
                self.detail = None;
            }
        }
        if let Some(menu) = &mut self.kanban.move_menu
            && menu.motion_open()
            && hidden(&menu.card)
        {
            menu.close();
            let focus = menu.focus.clone();
            self.retire_focus([focus]);
        }
        self.retire_hidden_move();
    }
}
