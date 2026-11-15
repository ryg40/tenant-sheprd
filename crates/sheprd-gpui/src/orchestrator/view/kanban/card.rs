//! One card as a tracker board draws it: a raised surface with its key, a
//! title of up to two lines, and the marks its prepared face holds.
use super::*;
use crate::config::{corners, mix};
use crate::orchestrator::view::{look::GREEN, pr_cells};

/// The card's own height inside its line, at the designed text size.
pub(super) const CARD_HEIGHT: f32 = 84.;
/// A card line: the card and the gap above it.
pub(super) const LINE_HEIGHT: f32 = CARD_HEIGHT + 6.;
const AVATAR: f32 = 16.;

impl OrchestratorView {
    pub(super) fn render_card(&self, card: &rows::Card, cx: &mut Context<Self>) -> AnyElement {
        let look = &self.look;
        let theme = &look.theme;
        let face = &card.face;
        let key = card.summary.key.clone();
        let selected = self.kanban.selected.as_ref() == Some(&card.domain.key);
        let hover_fill = mix(theme.surface, theme.foreground, 6);
        let hover_border = theme.muted;
        let hint: SharedString = if face.details.is_empty() {
            card.domain.item.title.clone().into()
        } else {
            format!("{}\n{}", card.domain.item.title, face.details).into()
        };
        let (foreground, surface) = (theme.foreground, theme.surface);
        let top = div()
            .h(px(look.row(16.)))
            .flex()
            .items_center()
            .gap_1()
            .text_size(look.small())
            .text_color(rgb(theme.subtext()))
            .child(self.card_handle(card, cx))
            .child(look.icon(
                if face.pull_request {
                    "icons/pull-request.svg"
                } else {
                    "icons/issue.svg"
                },
                12.,
                theme.muted,
            ))
            .child(
                div()
                    .flex_none()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(card.summary.identifier.clone()),
            )
            .children(face.rank.clone().map(|rank| {
                let fill = theme.primary();
                div()
                    .flex_none()
                    .min_w(px(16.))
                    .h(px(16.))
                    .px(px(4.))
                    .rounded_full()
                    .bg(rgb(fill))
                    .text_color(rgb(theme.text_on(fill)))
                    .text_size(look.tiny())
                    .font_weight(FontWeight::BOLD)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(rank)
            }))
            .children(face.pull.as_ref().map(|pull| {
                if pull.draft {
                    look.badge_in(fl!("kanban-card-draft"), theme.muted)
                } else {
                    look.badge(fl!("kanban-card-ready"), GREEN)
                }
            }))
            .child(div().flex_1())
            .child(
                div()
                    .flex_none()
                    .text_size(look.tiny())
                    .child(face.age.clone()),
            );
        let title = div()
            .flex_1()
            .min_h_0()
            .line_clamp(2)
            .text_ellipsis()
            .child(card.summary.title.clone());
        let chips = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .overflow_hidden()
            .children(face.labels.iter().map(|chip| {
                look.badge(chip.text.clone(), chip.hue)
                    .max_w(px(72.))
                    .truncate()
            }))
            .children(face.more.clone().map(|more| {
                div()
                    .flex_none()
                    .text_size(look.tiny())
                    .text_color(rgb(theme.subtext()))
                    .child(more)
            }));
        let marks = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .when(!card.suffix.is_empty(), |el| {
                el.child(
                    div()
                        .max_w(px(78.))
                        .truncate()
                        .text_size(look.tiny())
                        .text_color(rgb(theme.subtext()))
                        .child(card.suffix.clone()),
                )
            })
            .children(
                face.agent
                    .map(|status| look.dot(look.status_hue(status), 8.)),
            )
            .children(
                face.pull
                    .as_ref()
                    .and_then(|pull| pull.review.map(|hue| look.dot(look.hue(hue), 7.))),
            )
            .children(face.pull.as_ref().and_then(|pull| {
                pull.checks
                    .map(|state| look.dot(pr_cells::check_hue(look, state), 7.))
            }))
            .children(face.assignee.as_ref().map(|person| {
                let fill = mix(theme.background, look.hue(person.hue), 70);
                div()
                    .flex_none()
                    .size(px(AVATAR))
                    .rounded_full()
                    .bg(rgb(fill))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(AVATAR * 0.55))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(theme.text_on(fill)))
                    .child(person.text.clone())
            }));
        let bottom = div()
            .h(px(look.row(18.)))
            .flex()
            .items_center()
            .gap_1()
            .child(chips)
            .child(marks);
        let body = div()
            .id(card.element_id.clone())
            .role(Role::Button)
            .aria_label(card.name.clone())
            .tooltip(move |_, cx| {
                cx.new(|_| crate::menu::SettingsHint {
                    text: hint.clone(),
                    foreground,
                    surface,
                })
                .into()
            })
            .h(px(look.row(CARD_HEIGHT)))
            .w_full()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(8.))
            .py(px(6.))
            .overflow_hidden()
            .rounded(px(corners::CONTROL))
            .border_1()
            .border_color(rgb(theme.active))
            .bg(rgb(theme.surface))
            .shadow_sm()
            .cursor_pointer()
            .hover(move |style| style.bg(rgb(hover_fill)).border_color(rgb(hover_border)))
            .when(selected, |el| {
                el.bg(rgb(theme.primary_wash()))
                    .border_color(rgb(theme.primary()))
            })
            .child(top)
            .child(title)
            .child(bottom)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_kanban_key(&key, cx);
                this.focus_kanban_popup(window, cx);
            }));
        self.card_drag(body, card, cx)
    }
}
