//! C1 repository matrix: one virtual vertical list and one horizontal offset.
use super::*;
use rows::Line;
use sheprd_control_protocol::BoardColumn;

pub(super) const REPOSITORY_WIDTH: f32 = 188.;
pub(super) const COLUMN_WIDTH: f32 = 240.;
const ROW_HEIGHT: f32 = 30.;
/// One line of lane facts: small text with room at every text size.
const FACT_HEIGHT: f32 = 18.;
const BOARD_WIDTH: f32 = REPOSITORY_WIDTH + COLUMN_WIDTH * 4.;

impl OrchestratorView {
    pub(in crate::orchestrator::view) fn render_kanban(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.lane_drag_frame(window, cx);
        let theme = &self.look.theme;
        let ready = self.kanban.ready.clone();
        let title = div()
            .flex_none()
            .px_4()
            .py_3()
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .text_size(px(self.look.ui.size + 6.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(fl!("kanban-title")),
            )
            .children(ready.as_ref().map(|ready| {
                div()
                    .text_color(rgb(theme.subtext()))
                    .child(ready.loaded.clone())
            }));
        let search = div()
            .flex_none()
            .px_4()
            .pb_2()
            .flex()
            .gap_2()
            .child(div().flex_1().min_w_0().child(self.search.clone()))
            .child(
                self.look
                    .icon_button(
                        "kanban-refresh",
                        "icons/refresh.svg",
                        fl!("orchestrator-refresh"),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.ask_refresh();
                        cx.notify();
                    })),
            );
        let mut headings = div()
            .h(px(self.look.row(ROW_HEIGHT)))
            .flex_none()
            .flex()
            .child(
                div()
                    .w(px(REPOSITORY_WIDTH))
                    .flex_none()
                    .px_3()
                    .child(fl!("kanban-repository")),
            );
        for (index, column) in BoardColumn::ALL.iter().enumerate() {
            headings = headings.child(
                div()
                    .w(px(COLUMN_WIDTH))
                    .flex_none()
                    .px_3()
                    .flex()
                    .justify_between()
                    .child(rows::column(*column))
                    .children(
                        ready
                            .as_ref()
                            .and_then(|ready| ready.totals.get(index))
                            .cloned(),
                    ),
            );
        }
        let body = if ready.is_none() {
            div().p_4().child(fl!("kanban-pending")).into_any_element()
        } else if self.kanban.lines.is_empty() {
            div().p_4().child(fl!("kanban-empty")).into_any_element()
        } else {
            uniform_list(
                "kanban-rows",
                self.kanban.lines.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|index| this.render_kanban_line(index, cx))
                        .collect()
                }),
            )
            .track_scroll(&self.kanban.scroll)
            .flex_1()
            .min_h_0()
            .into_any_element()
        };
        let end = self.lane_target(div().h(px(18.)).flex_none(), None, usize::MAX, cx);
        let board = div()
            .id("kanban-horizontal")
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_x_scroll()
            .track_scroll(&self.kanban.horizontal)
            .child(
                div()
                    .w(px(BOARD_WIDTH))
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(headings)
                    .child(body)
                    .child(end),
            );
        let content = div()
            .relative()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .child(title)
            .child(search)
            .child(
                div()
                    .px_4()
                    .pb_2()
                    .text_color(rgb(theme.subtext()))
                    .child(fl!("kanban-read-only")),
            )
            .when(ready.as_ref().is_some_and(|ready| ready.partial), |el| {
                el.child(div().px_4().pb_2().child(fl!("kanban-partial")))
            })
            .child(div().px_4().pb_2().child(fl!("kanban-order-help")))
            .child(div().px_4().pb_2().child(fl!("kanban-rank-help")))
            .child(div().px_4().pb_2().child(fl!("kanban-move-help")))
            .when(self.kanban.save_failed, |el| {
                el.child(div().px_4().pb_2().child(fl!("kanban-order-unsaved")))
            })
            .children(
                self.kanban
                    .error
                    .as_ref()
                    .map(|error| div().px_4().pb_2().child(error.localize())),
            )
            .when(self.kanban.removed, |el| {
                el.child(div().px_4().pb_2().child(fl!("kanban-card-removed")))
            })
            .child(board)
            .children(
                ready
                    .as_ref()
                    .and_then(|ready| ready.history_notice.clone())
                    .map(|notice| {
                        div()
                            .flex_none()
                            .px_3()
                            .py_2()
                            .text_size(self.look.small())
                            .child(notice)
                    }),
            );
        content
            .children(self.render_kanban_popup(window, cx))
            .children(self.render_move_menu(window, cx))
            .children(self.render_move_dialog(window, cx))
            .into_any_element()
    }

    fn render_kanban_line(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(ready) = &self.kanban.ready else {
            return div().into_any_element();
        };
        let Some(line) = self.kanban.lines.get(index) else {
            return div().into_any_element();
        };
        let theme = &self.look.theme;
        // One uniform line height keeps the virtual list exact for 2,000 cards.
        let mut row = div()
            .h(px(self.look.row(card::LINE_HEIGHT)))
            .flex()
            .w_full()
            .overflow_hidden();
        match *line {
            Line::Header(lane_index) => {
                let lane = &ready.lanes[lane_index];
                let key = lane.key.canonical();
                let collapsed = self.kanban.collapsed.contains(&key);
                let hint = lane.hint.clone();
                let foreground = theme.foreground;
                let surface = theme.surface;
                let info = self.lane_facts([&lane.host, &lane.status]);
                let heading = div()
                    .h(px(self.look.row(ROW_HEIGHT)))
                    .flex()
                    .child(self.lane_handle(lane, cx))
                    .child(
                        div()
                            .id(lane.element_id.clone())
                            .role(Role::Button)
                            .aria_label(lane.name.clone())
                            .tooltip(move |_, cx| {
                                cx.new(|_| crate::menu::SettingsHint {
                                    text: hint.clone(),
                                    foreground,
                                    surface,
                                })
                                .into()
                            })
                            .w(px(REPOSITORY_WIDTH - 20.))
                            .flex_none()
                            .h_full()
                            .px_2()
                            .flex()
                            .items_center()
                            .gap_1()
                            .cursor_pointer()
                            .when(self.kanban.focused_lane.as_ref() == Some(&lane.key), |el| {
                                el.border_1().border_color(rgb(theme.primary()))
                            })
                            .child(self.look.icon(
                                if collapsed {
                                    "icons/chevron-right.svg"
                                } else {
                                    "icons/chevron-down.svg"
                                },
                                12.,
                                theme.muted,
                            ))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(lane.title.clone()),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if !this.kanban.collapsed.remove(&key) {
                                    this.kanban.collapsed.insert(key.clone());
                                }
                                this.kanban.focused_lane = order::parse(&key);
                                this.kanban.rebuild_lines();
                                window.focus(&this.focus, cx);
                                cx.notify();
                            })),
                    );
                row = row
                    .border_t_1()
                    .border_color(rgb(theme.active))
                    .bg(rgb(theme.surface))
                    .child(
                        div()
                            .w(px(REPOSITORY_WIDTH))
                            .flex_none()
                            .h_full()
                            .flex()
                            .flex_col()
                            .overflow_hidden()
                            .child(heading)
                            .child(info),
                    );
                for (count, column) in lane.counts.iter().zip(BoardColumn::ALL) {
                    row = row.child(
                        div()
                            .w(px(COLUMN_WIDTH))
                            .flex_none()
                            .h_full()
                            .px_3()
                            .pt_1()
                            .flex()
                            .flex_col()
                            .border_l_1()
                            .border_color(rgb(theme.active))
                            .text_color(rgb(theme.subtext()))
                            .child(
                                div()
                                    .h(px(self.look.row(ROW_HEIGHT - 4.)))
                                    .flex()
                                    .items_center()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(rows::column(*column)),
                            )
                            .child(div().text_size(self.look.small()).child(count.clone())),
                    );
                }
                return self.lane_target(row, Some(lane.key.clone()), index, cx);
            }
            Line::Facts(lane) => {
                let lane = &ready.lanes[lane];
                row = row.child(
                    div()
                        .w(px(REPOSITORY_WIDTH))
                        .flex_none()
                        .h_full()
                        .bg(rgb(theme.surface))
                        .pt_1()
                        .child(self.lane_facts([&lane.history, &lane.mapping, &lane.evidence])),
                );
                for _ in BoardColumn::ALL {
                    row = row.child(
                        div()
                            .w(px(COLUMN_WIDTH))
                            .flex_none()
                            .h_full()
                            .border_l_1()
                            .border_color(rgb(theme.active)),
                    );
                }
            }
            Line::Cards { lane, row: at } => {
                let lane = &ready.lanes[lane];
                row = row.child(
                    div()
                        .w(px(REPOSITORY_WIDTH))
                        .flex_none()
                        .h_full()
                        .bg(rgb(theme.surface)),
                );
                for (column_index, cell) in lane.cells.iter().enumerate() {
                    let mut column = div()
                        .w(px(COLUMN_WIDTH))
                        .flex_none()
                        .h_full()
                        .border_l_1()
                        .border_color(rgb(theme.active))
                        .px_2()
                        .pt(px(4.));
                    if let Some(card_index) = cell.get(at) {
                        column = column.child(self.render_card(&ready.cards[*card_index], cx));
                    } else if at == 0 {
                        column = column.child(
                            div()
                                .text_color(rgb(theme.subtext()))
                                .child(fl!("kanban-cell-empty")),
                        );
                    }
                    row = row.child(self.card_target(
                        column,
                        lane.key.clone(),
                        BoardColumn::ALL[column_index],
                        at,
                        cell.len(),
                        cx,
                    ));
                }
            }
        }
        let before = match *line {
            Line::Facts(lane) | Line::Cards { lane, .. } => {
                ready.lanes.get(lane + 1).map(|lane| lane.key.clone())
            }
            Line::Header(_) => None,
        };
        self.lane_target(row, before, index, cx)
    }

    /// Lane facts, one per line. Each line has its own scaled height, so the
    /// lines fit their uniform virtual line at every text size.
    fn lane_facts<'a>(&self, facts: impl IntoIterator<Item = &'a SharedString>) -> Div {
        div()
            .px_3()
            .flex()
            .flex_col()
            .text_size(self.look.small())
            .line_height(px(self.look.row(FACT_HEIGHT)))
            .text_color(rgb(self.look.theme.subtext()))
            .children(
                facts
                    .into_iter()
                    .filter(|text| !text.is_empty())
                    .map(|text| {
                        div()
                            .h(px(self.look.row(FACT_HEIGHT)))
                            .flex_none()
                            .min_w_0()
                            .truncate()
                            .child(text.clone())
                    }),
            )
    }
}
