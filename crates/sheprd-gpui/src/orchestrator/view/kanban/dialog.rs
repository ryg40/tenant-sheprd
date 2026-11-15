//! The native exact-change dialog owns the one-shot preparation capability.
use super::*;
use crate::{config::corners, i18n::errors::Localize, motion};
use sheprd_control_protocol::BoardColumn;
use sheprd_orchestrator::{
    kanban::{
        history::Record,
        identity::{LaneKey, Origin},
        moves::{MovePlan, Mutation},
    },
    service::{MovePhase, MoveTicket, Moves},
};

pub(super) struct Dialog {
    body: Vec<(SharedString, SharedString)>,
    card: CardKey,
    confirm_focus: FocusHandle,
    focus: FocusHandle,
    motion: motion::Toggle,
    moves: Moves,
    operation: uuid::Uuid,
    origin: Origin,
    phase: MovePhase,
    status: SharedString,
    ticket: Option<MoveTicket>,
    worker: String,
}

impl OrchestratorView {
    pub(crate) fn propose_card(
        &mut self,
        item: &str,
        lane: &LaneKey,
        to: BoardColumn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), error::Error> {
        if self.kanban.move_dialog.is_some()
            || self.kanban.move_menu_open()
            || self.dialog.is_some()
            || self.confirm.is_some()
            || self.merge_open
        {
            return Err(error::Error::Busy);
        }
        if self.kanban.prepared != Some(self.kanban.revision) {
            return Err(error::Error::Pending);
        }
        if self
            .kanban
            .ready
            .as_ref()
            .is_none_or(|ready| !ready.roster.contains(lane))
            || !self.lane_shown(lane)
        {
            return Err(error::Error::UnknownLane);
        }
        let card = self
            .kanban
            .card(item)
            .cloned()
            .ok_or(error::Error::UnknownCard)?;
        if &card.domain.key.lane != lane {
            return Err(sheprd_orchestrator::kanban::Error::CrossLane.into());
        }
        if !move_status::available(&card.domain.item) {
            return Err(error::Error::Unsupported);
        }
        let repo = self.kanban_repo_of(item).ok_or(error::Error::UnknownCard)?;
        let observed = Record {
            item: card.domain.item.clone(),
            facts: card.domain.facts.clone(),
        };
        let proposal = sheprd_orchestrator::kanban::moves::Proposal {
            // UI revisions fence proposals; service generations fence retained plans.
            board_generation: 0,
            card: card.domain.key.clone(),
            destination: to,
            destination_lane: lane.clone(),
        };
        let repo = &self.repos[repo];
        let moves = repo
            .service
            .as_ref()
            .ok_or(error::Error::Unsupported)?
            .moves()
            .clone();
        let generation = moves.generation();
        let context = sheprd_orchestrator::kanban::moves::Context {
            account: String::new(),
            account_generation: 0,
            api: sheprd_orchestrator::forgejo::moves::api_root(&card.domain.item.key.source)
                .map_err(error::Error::Backend)?,
            board_generation: 0,
            mapping: card.domain.mapping.clone(),
            mapping_generation: 0,
            route: sheprd_orchestrator::kanban::identity::Route {
                origin: card.domain.route.origin.clone(),
                worker: generation,
            },
            source_generation: generation,
        };
        let scheme = sheprd_orchestrator::forgejo::moves::Scheme::of_source(
            repo.snapshot.repo.as_ref(),
            &card.domain.item.key.source,
        );
        #[cfg(not(test))]
        let ticket = moves
            .prepare_forgejo_dialog(proposal, context, scheme, observed)
            .map_err(error::Error::Backend)?;
        #[cfg(test)]
        let ticket = {
            let _ = scheme;
            let peer = self
                .kanban
                .test_move
                .clone()
                .ok_or(error::Error::Unsupported)?;
            let fresh = peer.prepare(context, observed.item.clone());
            moves
                .configure(lane.clone(), fresh.context.clone())
                .map_err(error::Error::Backend)?;
            moves
                .prepare(proposal, observed, move |_| {
                    peer.prepared();
                    Ok(fresh)
                })
                .map_err(error::Error::Backend)?
        };
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut motion = motion::Toggle::default();
        motion.set(true, motion::now(), false);
        self.kanban.move_dialog = Some(Dialog {
            body: vec![
                (
                    fl!("kanban-move-item").into(),
                    format!(
                        "{}: {}",
                        card.domain.item.identifier, card.domain.item.title
                    )
                    .into(),
                ),
                (
                    fl!("kanban-move-host").into(),
                    card.domain.item.key.source.host.clone().into(),
                ),
                (
                    fl!("kanban-move-repository").into(),
                    card.domain.item.key.source.repository.clone().into(),
                ),
                (
                    fl!("kanban-move-menu").into(),
                    fl!(
                        "kanban-move-columns",
                        from = rows::column(card.domain.placement.column),
                        to = rows::column(to)
                    )
                    .into(),
                ),
            ],
            card: card.domain.key,
            confirm_focus: cx.focus_handle(),
            focus,
            motion,
            moves,
            operation: ticket.operation(),
            origin: card.domain.route.origin,
            phase: MovePhase::Preparing,
            status: fl!("kanban-move-preparing", column = rows::column(to)).into(),
            ticket: Some(ticket),
            worker: repo.key.clone(),
        });
        cx.notify();
        Ok(())
    }

    /// Pending projection is not authority to confirm, but does not retire an unrelated plan.
    fn move_dialog_route(&self) -> Result<(), error::Error> {
        let Some(dialog) = &self.kanban.move_dialog else {
            return Ok(());
        };
        let stale = || error::Error::Model(sheprd_orchestrator::kanban::Error::StaleMove);
        if !self.repos.iter().any(|repo| {
            repo.key == dialog.worker
                && repo.service.as_ref().is_some_and(|service| {
                    service.moves().generation() == dialog.moves.generation()
                })
        }) {
            return Err(stale());
        }
        // A lane the selection hides confirms nothing.
        if !self.lane_shown(&dialog.card.lane) {
            return Err(stale());
        }
        if self.kanban.prepared != Some(self.kanban.revision) {
            return Err(error::Error::Pending);
        }
        let ready = self.kanban.ready.as_ref().ok_or_else(stale)?;
        // Follow the selected record even if its spelling changes or it is not displayed.
        let selected = ready
            .selected_routes
            .get(&(dialog.card.lane.clone(), dialog.card.item.native_id.clone()))
            .ok_or_else(stale)?;
        if selected.origin != dialog.origin
            || ready
                .routes
                .get(selected.worker)
                .is_none_or(|route| route.worker != dialog.worker)
        {
            return Err(stale());
        }
        Ok(())
    }

    pub(in crate::orchestrator::view) fn poll_move_dialog(&mut self, cx: &mut Context<Self>) {
        let route = self.move_dialog_route();
        let Some(dialog) = &mut self.kanban.move_dialog else {
            return;
        };
        if !dialog.motion.is_open() {
            return;
        }
        if let Err(error) = route {
            if matches!(error, error::Error::Pending) {
                return;
            }
            if dialog.ticket.take().is_some() {
                dialog.phase = MovePhase::Refused(Arc::new(
                    sheprd_orchestrator::kanban::Error::StaleMove.into(),
                ));
                dialog.status = error.localize().into();
                cx.notify();
            }
            return;
        }
        let status = dialog
            .moves
            .snapshot()
            .into_iter()
            .find(|s| s.operation == dialog.operation && s.card == dialog.card);
        if status.is_none() {
            dialog.ticket = None;
            dialog.phase = MovePhase::Refused(Arc::new(
                sheprd_orchestrator::kanban::Error::StaleMove.into(),
            ));
            dialog.status = sheprd_orchestrator::kanban::Error::StaleMove
                .localize()
                .into();
            cx.notify();
            return;
        }
        if let Some(status) = status {
            let text: SharedString = move_status::label(&status).into();
            if text == dialog.status {
                return;
            }
            dialog.status = text;
            if let MovePhase::AwaitingConfirmation(plan) = &status.phase {
                dialog.body = plan_rows(plan);
            }
            if let MovePhase::Finished(completion) = &status.phase {
                if !completion.applied.is_empty() {
                    dialog.body.push((
                        SharedString::default(),
                        fl!(
                            "kanban-move-applied",
                            changes = completion
                                .applied
                                .iter()
                                .map(|step| move_status::mutation(&completion.plan, step))
                                .collect::<Vec<_>>()
                                .join("; ")
                        )
                        .into(),
                    ));
                }
                if let Some(error) = &completion.read_error {
                    dialog.body.push((
                        SharedString::default(),
                        fl!("kanban-move-read-failed", error = error.localize()).into(),
                    ));
                }
            }
            dialog.phase = status.phase;
            cx.notify();
        }
    }

    /// The only GUI owner of preparation authority, reached by the native button.
    fn move_ticket_clicked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Recheck before taking the capability, including before a pending projection settles.
        if self.move_dialog_route().is_err() {
            self.poll_move_dialog(cx);
            return;
        }
        let Some(dialog) = &mut self.kanban.move_dialog else {
            return;
        };
        if !dialog.motion.is_open() {
            return;
        }
        let Some(ticket) = dialog.ticket.take() else {
            return;
        };
        #[cfg(not(test))]
        let result = dialog
            .moves
            .confirm(ticket)
            .and_then(|approved| dialog.moves.submit_forgejo_dialog(approved));
        #[cfg(test)]
        let result = dialog.moves.confirm(ticket).and_then(|approved| {
            let read = self
                .kanban
                .test_move
                .clone()
                .ok_or(sheprd_orchestrator::kanban::Error::ReadOnlyMove)?;
            let write = read.clone();
            dialog.moves.submit(
                approved,
                move || read.read(),
                move |plan, step| write.mutate(plan, step),
            )
        });
        if let Err(error) = result {
            dialog.status = fl!("kanban-move-refused", error = error.localize()).into();
            dialog.phase = MovePhase::Refused(Arc::new(error));
        }
        window.focus(&dialog.focus, cx);
        cx.notify();
    }

    #[cfg(test)]
    pub(in crate::orchestrator::view) fn fixture_move(
        &mut self,
        peer: super::super::tests::kanban_moves::Peer,
    ) {
        self.kanban.test_move = Some(peer);
    }
    #[cfg(test)]
    pub(in crate::orchestrator::view) fn select_move_for_test(&mut self, card: CardKey) {
        self.kanban.selected = Some(card);
    }
    #[cfg(test)]
    pub(in crate::orchestrator::view) fn drop_move_for_test(&mut self) {
        self.kanban.move_dialog = None;
    }
    #[cfg(test)]
    pub(in crate::orchestrator::view) fn confirm_move_for_test(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_ticket_clicked(window, cx);
    }
    /// Whether the open move dialog shows its confirming button enabled:
    /// the plan is prepared, its capability held, and the board current.
    pub(in crate::orchestrator::view) fn move_confirmable(&self) -> bool {
        self.kanban.move_dialog.as_ref().is_some_and(|dialog| {
            dialog.motion.is_open()
                && dialog.ticket.is_some()
                && self.kanban.prepared == Some(self.kanban.revision)
                && matches!(dialog.phase, MovePhase::AwaitingConfirmation(_))
        })
    }
    #[cfg(test)]
    pub(in crate::orchestrator::view) fn close_move_dialog_for_test(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_move_dialog(window, cx);
    }

    pub(super) fn restore_move_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.kanban.popup.is_shown() {
            self.focus_kanban_popup(window, cx);
        } else {
            window.focus(&self.focus, cx);
        }
    }

    pub(super) fn move_dialog_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        if event.keystroke.key == "escape" {
            self.close_move_dialog(window, cx);
            return;
        }
        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
            if self
                .kanban
                .move_dialog
                .as_ref()
                .is_some_and(|d| d.confirm_focus.is_focused(window))
            {
                self.move_ticket_clicked(window, cx);
            } else {
                self.close_move_dialog(window, cx);
            }
            return;
        }
        if event.keystroke.key == "tab"
            && let Some(dialog) = &self.kanban.move_dialog
        {
            if dialog.confirm_focus.is_focused(window)
                || !matches!(dialog.phase, MovePhase::AwaitingConfirmation(_))
            {
                window.focus(&dialog.focus, cx);
            } else {
                window.focus(&dialog.confirm_focus, cx);
            }
        }
    }

    /// Withdraws a move not yet confirmed whose lane the selection now
    /// hides. A submitted move holds no ticket and is left to finish.
    pub(super) fn retire_hidden_move(&mut self) {
        let Some(dialog) = &mut self.kanban.move_dialog else {
            return;
        };
        if dialog.ticket.is_none() || self.repo_select.shows(Some(&dialog.card.lane.canonical())) {
            return;
        }
        dialog.ticket = None;
        if let Some(service) = self
            .repos
            .iter()
            .find(|repo| repo.key == dialog.worker)
            .and_then(|repo| repo.service.as_ref())
        {
            service.moves().cancel(&dialog.card);
        }
        dialog.motion.set(false, motion::now(), false);
        let handles = [dialog.focus.clone(), dialog.confirm_focus.clone()];
        self.retire_focus(handles);
    }

    fn close_move_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dialog) = &mut self.kanban.move_dialog {
            dialog.ticket = None;
            if let Some(service) = self
                .repos
                .iter()
                .find(|repo| repo.key == dialog.worker)
                .and_then(|repo| repo.service.as_ref())
            {
                service.moves().cancel(&dialog.card);
            }
            dialog.motion.set(false, motion::now(), false);
        }
        self.restore_move_focus(window, cx);
        cx.notify();
    }

    pub(super) fn render_move_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.kanban.move_dialog.as_ref()?;
        let now = motion::now();
        if !dialog.motion.is_open() && !dialog.motion.moving(now) {
            self.kanban.move_dialog = None;
            return None;
        }
        let shown = dialog.motion.shown(now);
        if dialog.motion.moving(now) {
            window.request_animation_frame();
        }
        let can_confirm = self.move_confirmable();
        let theme = &self.look.theme;
        let mut body = div()
            .id("kanban-move-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3();
        for (name, value) in &dialog.body {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_color(rgb(theme.subtext())).child(name.clone()))
                    .child(value.clone()),
            );
        }
        body = body
            .child(dialog.status.clone())
            .child(fl!("kanban-move-original"))
            .child(fl!("kanban-move-warning"));
        let actions = div()
            .flex_none()
            .flex()
            .justify_end()
            .gap_2()
            .child(
                self.look
                    .button("kanban-move-cancel", fl!("kanban-move-cancel"), false)
                    .track_focus(&dialog.focus)
                    .tab_index(0)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.close_move_dialog(window, cx)),
                    ),
            )
            .when(can_confirm, |el| {
                el.child(
                    self.look
                        .button("kanban-move-confirm", fl!("kanban-move-confirm"), true)
                        .track_focus(&dialog.confirm_focus)
                        .tab_index(0)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.move_ticket_clicked(window, cx)),
                        ),
                )
            });
        Some(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .bg(rgba(0x00000080))
                .opacity(shown)
                .flex()
                .items_center()
                .justify_center()
                .p_3()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .on_scroll(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .id("kanban-move-dialog")
                        .role(Role::Dialog)
                        .aria_label(fl!("kanban-move-title"))
                        .key_context("KanbanMoveDialog")
                        .on_key_down(cx.listener(Self::move_dialog_key))
                        .w(px(560.))
                        .max_w_full()
                        .h(relative(0.85))
                        .min_h_0()
                        .p_4()
                        .gap_3()
                        .mt(px((1. - shown) * 12.))
                        .flex()
                        .flex_col()
                        .rounded(px(corners::PANEL))
                        .bg(rgb(theme.background))
                        .border_1()
                        .border_color(rgb(theme.active))
                        .shadow_lg()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(fl!("kanban-move-title")),
                        )
                        .child(body)
                        .child(actions),
                )
                .into_any_element(),
        )
    }
}

fn plan_rows(plan: &MovePlan) -> Vec<(SharedString, SharedString)> {
    let item = plan.original().item;
    let context = plan.context();
    let mut rows = vec![
        (
            fl!("kanban-move-provider").into(),
            item.key.source.provider.as_str().to_owned().into(),
        ),
        (
            fl!("kanban-move-host").into(),
            context.api.as_str().to_owned().into(),
        ),
        (
            fl!("kanban-move-repository").into(),
            item.key.source.repository.into(),
        ),
        (
            fl!("kanban-move-item").into(),
            format!("{}: {}", item.identifier, item.title).into(),
        ),
        (
            fl!("kanban-move-account").into(),
            context.account.clone().into(),
        ),
        (
            fl!("kanban-move-menu").into(),
            fl!(
                "kanban-move-columns",
                from = rows::column(plan.original_column()),
                to = rows::column(plan.destination())
            )
            .into(),
        ),
    ];
    for mutation in plan.mutations() {
        rows.push((
            SharedString::default(),
            move_status::mutation(plan, mutation).into(),
        ));
    }
    if !plan
        .mutations()
        .iter()
        .any(|step| matches!(step, Mutation::SetState(_)))
    {
        rows.push((
            SharedString::default(),
            fl!("kanban-move-unchanged-state", state = item.state).into(),
        ));
    }
    rows.push((SharedString::default(), fl!("kanban-move-unchanged").into()));
    rows
}
