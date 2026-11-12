//! Typed lane-header drags. Pointer movement never changes the saved order.
use super::*;
use crate::motion;
use sheprd_orchestrator::kanban::identity::LaneKey;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::orchestrator::view) struct LaneDrag {
    pub board: EntityId,
    pub lane: LaneKey,
    pub revision: u64,
}

#[derive(Default)]
pub(in crate::orchestrator::view) struct State {
    pub active: Option<LaneDrag>,
    gaps: HashMap<Option<LaneKey>, motion::Toggle>,
    pending: Option<LaneKey>,
    placements: Vec<(LaneKey, motion::Toggle)>,
    target: Option<Option<LaneKey>>,
}

impl State {
    pub fn cancel(&mut self) {
        self.active = None;
        // A committed drop still uses the old rows until preparation installs it.
        if self.pending.is_some() {
            return;
        }
        self.target = None;
        for gap in self.gaps.values_mut() {
            gap.set(false, motion::now(), false);
        }
    }

    pub fn placed(&mut self, lane: LaneKey) {
        self.active = None;
        self.pending = Some(lane);
    }

    pub fn installed(&mut self) {
        let Some(lane) = self.pending.take() else {
            return;
        };
        let now = motion::now();
        self.placements.retain(|(_, gap)| gap.moving(now));
        if let Some(target) = self.target.take()
            && let Some(mut gap) = self.gaps.remove(&target)
        {
            // Move the existing space with the new rows, reversing from its
            // current displacement. Earlier interrupted placements keep fading.
            gap.set(false, now, false);
            self.placements.push((lane, gap));
        }
        self.cancel();
    }

    pub fn failed(&mut self) {
        self.pending = None;
        self.cancel();
    }

    fn target(&mut self, target: Option<Option<LaneKey>>) {
        if self.target == target {
            return;
        }
        self.target = target.clone();
        self.gaps
            .retain(|_, gap| gap.is_open() || gap.moving(motion::now()));
        for (key, gap) in &mut self.gaps {
            gap.set(target.as_ref() == Some(key), motion::now(), false);
        }
        if let Some(key) = target {
            self.gaps
                .entry(key)
                .or_default()
                .set(true, motion::now(), false);
        }
    }
}

struct Preview {
    look: crate::orchestrator::Look,
    title: SharedString,
}

impl Render for Preview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .p_3()
            .w(px(280.))
            .bg(rgb(self.look.theme.surface))
            .text_color(rgb(self.look.theme.foreground))
            .border_1()
            .border_color(rgb(self.look.theme.primary()))
            .shadow_lg()
            .child(self.title.clone())
    }
}

impl OrchestratorView {
    pub(super) fn lane_handle(&self, lane: &rows::Lane, cx: &mut Context<Self>) -> AnyElement {
        let key = lane.key.clone();
        let title = lane.title.clone();
        let look = self.look.clone();
        let view = cx.entity();
        let drag = LaneDrag {
            board: cx.entity_id(),
            lane: key.clone(),
            revision: self.kanban.revision,
        };
        let dots = div()
            .w(px(8.))
            .flex_none()
            .flex()
            .flex_wrap()
            .gap(px(2.))
            .children((0..6).map(|_| {
                div()
                    .size(px(2.))
                    .rounded_full()
                    .bg(rgb(self.look.theme.muted))
            }));
        div()
            .id(SharedString::from(format!(
                "lane-handle-{}",
                key.canonical()
            )))
            .role(Role::Button)
            .aria_label(fl!("kanban-lane-handle", source = key.canonical()))
            .w(px(20.))
            .h_full()
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .cursor(CursorStyle::OpenHand)
            .child(dots)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.kanban.focused_lane = Some(key.clone());
                window.focus(&this.focus, cx);
                cx.notify();
            }))
            .on_drag(drag, move |drag, _, window, cx| {
                view.update(cx, |this, cx| {
                    this.kanban.drag.cancel();
                    this.kanban.drag.active = Some(drag.clone());
                    this.kanban.focused_lane = Some(drag.lane.clone());
                    window.focus(&this.focus, cx);
                    cx.notify();
                });
                cx.new(|_| Preview {
                    look: look.clone(),
                    title: title.clone(),
                })
            })
            .into_any_element()
    }

    pub(super) fn lane_target(
        &self,
        row: Div,
        before: Option<LaneKey>,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = cx.entity();
        let board = cx.entity_id();
        let target = before.clone();
        let hover_target = before.clone();
        let now = motion::now();
        let state = &self.kanban.drag;
        let lane_index = match self.kanban.lines.get(index) {
            Some(rows::Line::Header(lane) | rows::Line::Cards { lane, .. }) => *lane,
            None => usize::MAX,
        };
        let rank = |key: Option<&LaneKey>| {
            key.and_then(|key| {
                self.kanban
                    .ready
                    .as_ref()?
                    .lanes
                    .iter()
                    .position(|lane| &lane.key == key)
            })
            .unwrap_or(usize::MAX)
        };
        // Uniform-list slots keep their measured height. Translate the complete rows
        // and their hit regions instead of shrinking a header inside its slot.
        let mut offset: f32 = state
            .gaps
            .iter()
            .filter(|(key, _)| rank(key.as_ref()) <= lane_index)
            .map(|(_, gap)| 6. * gap.shown(now))
            .sum();
        let mut fade: f32 = 0.;
        for (lane, placement) in &state.placements {
            let at = rank(Some(lane));
            let shown = placement.shown(now);
            if at <= lane_index {
                offset += 6. * shown;
            }
            if at == lane_index {
                fade = fade.max(shown);
            }
        }
        let moving = state.gaps.values().any(|gap| gap.moving(now))
            || state.placements.iter().any(|(_, gap)| gap.moving(now));
        let row = row
            .relative()
            .top(px(offset))
            .when(moving, |el| {
                el.on_children_prepainted(|_, window, _| window.request_animation_frame())
            })
            .id(("lane-target", index))
            .when(fade > 0., |el| {
                el.bg(rgb(self.look.theme.primary_wash()).opacity(fade))
            })
            .can_drop(move |value, _, cx| {
                value.downcast_ref::<LaneDrag>().is_some_and(|drag| {
                    let this = view.read(cx);
                    this.validate_lane_drag(drag, board).is_ok()
                        && target.as_ref() != Some(&drag.lane)
                        && target.as_ref().is_none_or(|key| {
                            this.kanban
                                .ready
                                .as_ref()
                                .is_some_and(|ready| ready.roster.contains(key))
                        })
                })
            })
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<LaneDrag>, _, cx| {
                    if event.bounds.contains(&event.event.position)
                        && this
                            .validate_lane_drag(event.drag(cx), cx.entity_id())
                            .is_ok()
                    {
                        this.kanban.drag.target(Some(hover_target.clone()));
                        cx.notify();
                    }
                }),
            )
            .on_drop(cx.listener(move |this, drag: &LaneDrag, window, cx| {
                let _ = this.drop_lane(drag, before.as_ref(), cx);
                this.kanban.drag.cancel();
                window.focus(&this.focus, cx);
                cx.notify();
            }));
        // The translated child keeps the uniform slot's size and moves hit tests too.
        div().w_full().child(row).into_any_element()
    }

    pub(super) fn lane_drag_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !cx.has_active_drag() && self.kanban.drag.active.is_some() {
            self.kanban.drag.cancel();
        }
        if self.kanban.drag.active.is_none() {
            return;
        }
        let handle = self.kanban.scroll.0.borrow().base_handle.clone();
        let bounds = handle.bounds().intersect(&self.kanban.horizontal.bounds());
        let pointer = window.mouse_position();
        if !bounds.contains(&pointer) {
            self.kanban.drag.target(None);
            window.request_animation_frame();
            return;
        }
        // Only the board's own scroll handles move; never the window or desktop.
        window.request_animation_frame();
        let dy = if pointer.y < bounds.top() + px(30.) {
            5.
        } else if pointer.y > bounds.bottom() - px(30.) {
            -5.
        } else {
            0.
        };
        let dx = if pointer.x < bounds.left() + px(30.) {
            5.
        } else if pointer.x > bounds.right() - px(30.) {
            -5.
        } else {
            0.
        };
        if dy != 0. || dx != 0. {
            let offset = handle.offset();
            handle.set_offset(point(
                offset.x,
                (offset.y + px(dy)).clamp(-handle.max_offset().y, px(0.)),
            ));
            let h = &self.kanban.horizontal;
            h.set_offset(point(
                (h.offset().x + px(dx)).clamp(-h.max_offset().x, px(0.)),
                h.offset().y,
            ));
            window.request_animation_frame();
        }
    }
}
