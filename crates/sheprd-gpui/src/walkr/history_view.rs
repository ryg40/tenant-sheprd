//! The saved sessions in the Walkr section: the list, and the detail of a
//! picked row as plain text elements. Nothing here is a link, Markdown, or
//! HTML, and nothing is opened or run: session files are untrusted data.
// i18n: migrated
use super::{
    history::{Detail, Error, OptionBody, Row, RowState, Status},
    sessions::Shown,
    waiting_view::remote_line,
};
use crate::{SheprdWindow, direction::Logical, i18n::fl, motion};
use gpui::{prelude::*, *};
use std::{cell::Cell, rc::Rc};

/// Below this width, in pixels, the list goes above the content.
const NARROW: f32 = 720.;
/// The list's width beside the content, and its height above it.
const LIST_WIDTH: f32 = 280.;
const LIST_HEIGHT: f32 = 200.;

/// When a session was made, in this computer's time zone.
pub(super) fn local_time(row_time: Option<chrono::DateTime<chrono::Utc>>) -> String {
    row_time.map_or_else(String::new, |time| {
        time.with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string()
    })
}

fn status_text(status: Status) -> Option<SharedString> {
    match status {
        Status::Open => Some(fl!("walkr-row-open").into()),
        Status::Submitted => Some(fl!("walkr-row-submitted").into()),
        Status::Other => None,
    }
}

fn error_text(error: &Error) -> SharedString {
    match error {
        Error::TimedOut => fl!("walkr-detail-timed-out"),
        Error::TooLarge => fl!("walkr-detail-too-large"),
        Error::NotRegular | Error::InvalidId => fl!("walkr-detail-refused"),
        Error::Missing => fl!("walkr-detail-missing"),
        Error::Invalid(_) => fl!("walkr-detail-invalid"),
        Error::Schema => fl!("walkr-detail-schema"),
        Error::SessionMismatch => fl!("walkr-detail-mismatch"),
        Error::UnknownVersion(version) => fl!("walkr-detail-version", version = version),
        Error::NoHome | Error::Open(_) | Error::Read(_) => fl!("walkr-detail-failed"),
    }
    .into()
}

/// Half stars as Walkr writes them, in stars: 7 is 3.5.
fn stars(value: i64) -> String {
    let whole = value.div_euclid(2);
    if value.rem_euclid(2) == 0 {
        whole.to_string()
    } else {
        format!("{whole}.5")
    }
}

/// Fades content in as it takes the place of other content.
fn fade_in(element: Div, id: impl Into<ElementId>) -> AnyElement {
    element
        .with_animation(
            id,
            Animation::new(motion::duration(motion::ENTER)),
            |element, k| {
                let k = if cfg!(test) { 1. } else { k };
                element.opacity(1. - (1. - k).powi(3))
            },
        )
        .into_any_element()
}

/// Records the section's width as it lays out, and draws again when that
/// moves it across [`NARROW`].
fn measure(width: Rc<Cell<f32>>) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            let now = f32::from(bounds.size.width);
            let before = width.replace(now);
            if (before == 0. && now > 0. && now < NARROW) || (before >= NARROW) != (now >= NARROW) {
                window.refresh();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

impl SheprdWindow {
    /// The list beside `content`, or above it in a narrow section. `content`
    /// is the page or the state of the run; a picked row that is not live
    /// takes its place with its detail.
    pub(super) fn render_walkr_history(
        &self,
        content: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let width = self.walkr.sessions.width.get();
        let narrow = width > 0. && width < NARROW;
        let content = match (self.walkr.detail_shown(), self.walkr.sessions.pick()) {
            (true, Some(pick)) => fade_in(
                self.walkr_detail(cx),
                SharedString::from(format!("walkr-detail-{pick}")),
            ),
            _ => fade_in(
                div().size_full().flex().flex_col().child(content),
                "walkr-content",
            ),
        };
        let list = self.walkr_list(cx);
        let list = if narrow {
            list.w_full().h(px(LIST_HEIGHT)).border_b_1()
        } else {
            list.h_full().w(px(LIST_WIDTH)).border_r_1()
        };
        div()
            .relative()
            .flex()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .when(narrow, |body| body.flex_col())
            .when(!narrow, |body| body.row())
            .child(measure(self.walkr.sessions.width.clone()))
            .child(list.flex_none())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(content),
            )
            .into_any_element()
    }

    fn walkr_list(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = &self.theme;
        let sessions = &self.walkr.sessions;
        let live = self.walkr.live_session();
        let note = if let Some(_error) = sessions.error() {
            Some(fl!("walkr-history-worker-failed"))
        } else if sessions.list_timed_out() {
            Some(fl!("walkr-history-timed-out"))
        } else if sessions.rows().is_empty() {
            Some(if sessions.listed() {
                fl!("walkr-history-empty")
            } else {
                fl!("walkr-history-loading")
            })
        } else {
            None
        };
        let waiting = self.walkr_waiting(cx);
        let rows = sessions.rows().iter().enumerate().map(|(index, row)| {
            let picked = sessions.pick() == Some(row.id.as_str());
            self.walkr_row(index, row, live == Some(row.id.as_str()), picked, cx)
        });
        div()
            .id("walkr-history")
            .debug_selector(|| "walkr-history".into())
            .flex()
            .flex_col()
            .min_h_0()
            .overflow_y_scroll()
            .bg(rgb(theme.background))
            .border_color(rgb(theme.active))
            .child(
                div()
                    .px_3()
                    .py_2()
                    .text_color(rgb(theme.muted))
                    .child(fl!("walkr-history-title")),
            )
            .child(waiting)
            .children(note.map(|note| div().px_3().py_1().text_color(rgb(theme.muted)).child(note)))
            .children(rows)
    }

    fn walkr_row(
        &self,
        index: usize,
        row: &Row,
        live: bool,
        picked: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = &self.theme;
        let (title, status): (SharedString, Option<SharedString>) = match &row.state {
            RowState::Read { title, status, .. } => (
                title
                    .clone()
                    .filter(|title| !title.trim().is_empty())
                    .map_or_else(|| fl!("walkr-row-untitled").into(), SharedString::from),
                status.and_then(status_text),
            ),
            RowState::Unreadable => (fl!("walkr-row-unreadable").into(), None),
            RowState::TimedOut => (
                fl!("walkr-row-unreadable").into(),
                Some(fl!("walkr-row-timed-out").into()),
            ),
        };
        let id = row.id.clone();
        let time = local_time(row.created);
        let waits = matches!(
            row.state,
            RowState::Read {
                status: Some(Status::Open),
                ..
            }
        );
        // The state of the page of this session, opened from this
        // computer's Walkr server.
        let waiting = &self.walkr.waiting;
        let page = waiting
            .remote
            .as_ref()
            .filter(|remote| remote.machine.is_local() && remote.id == row.id)
            .map(|remote| {
                let host = waiting.local().map_or("", |host| host.label.as_str());
                div()
                    .text_size(px(self.config.ui.size - 1.))
                    .text_color(rgb(theme.muted))
                    .child(remote_line(&remote.state, host, true))
            });
        div()
            .id(("walkr-row", index))
            .debug_selector(move || format!("walkr-row-{index}"))
            .role(Role::Button)
            .aria_label(title.clone())
            .px_3()
            .py_2()
            .flex()
            .flex_col()
            .gap_0p5()
            .cursor_pointer()
            .when(picked, |row| row.bg(rgb(theme.active)))
            .hover(|style| style.bg(rgb(theme.active)))
            .child(div().min_w_0().truncate().child(title))
            .child(
                div()
                    .row()
                    .gap_2()
                    .min_w_0()
                    .text_size(px(self.config.ui.size - 1.))
                    .text_color(rgb(theme.muted))
                    .child(time)
                    .children(status)
                    .when(live, |line| {
                        line.child(
                            div()
                                .text_color(rgb(theme.ink(theme.palette[2])))
                                .child(fl!("walkr-row-live")),
                        )
                    }),
            )
            .children(page)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.walkr.waiting.unmatched = false;
                this.pick_local_walkr_session(id.clone(), waits, window, cx);
                cx.notify();
            }))
            .into_any_element()
    }

    /// The picked session as plain text, or why it is not shown.
    fn walkr_detail(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        let body = div()
            .id("walkr-detail")
            .track_scroll(&self.walkr.sessions.scroll)
            .debug_selector(|| "walkr-detail".into())
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .p_5()
            .bg(rgb(theme.background));
        let body = match self.walkr.sessions.detail() {
            Some(Shown::Detail(detail)) => self.walkr_detail_body(body, detail, cx),
            Some(Shown::Failed(error)) => {
                body.text_color(rgb(theme.muted)).child(error_text(error))
            }
            Some(Shown::Loading) | None => body
                .text_color(rgb(theme.muted))
                .child(fl!("walkr-detail-loading")),
        };
        div().size_full().flex().flex_col().child(body)
    }

    fn walkr_heading(&self, text: SharedString) -> Div {
        div()
            .pt_2()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(self.theme.foreground))
            .child(text)
    }

    fn walkr_muted(&self, text: impl Into<SharedString>) -> Div {
        div().text_color(rgb(self.theme.muted)).child(text.into())
    }

    fn walkr_detail_body(
        &self,
        body: Stateful<Div>,
        detail: &Detail,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = &self.theme;
        let mut meta = vec![local_time(detail.created)];
        meta.extend(status_text(detail.status).map(String::from));
        if !detail.workflow.is_empty() {
            meta.push(detail.workflow.clone());
        }
        let title = if detail.title.trim().is_empty() {
            fl!("walkr-row-untitled")
        } else {
            detail.title.clone()
        };
        let body = body
            .child(
                div()
                    .text_size(px(self.config.ui.size + 2.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(self.walkr_muted(meta.join(" · ")))
            .child(self.walkr_heading(fl!("walkr-detail-brief").into()))
            .child(div().child(detail.brief.clone()));
        let body = body.when(!detail.clarifications.is_empty(), |body| {
            body.child(self.walkr_heading(fl!("walkr-detail-questions").into()))
                .children(detail.clarifications.iter().map(|(question, answer)| {
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(question.clone()),
                        )
                        .child(div().child(answer.clone()))
                }))
        });
        let body = body.when(detail.clarifications_omitted > 0, |body| {
            body.child(self.walkr_muted(fl!(
                "walkr-detail-questions-omitted",
                count = detail.clarifications_omitted
            )))
        });
        let pick = detail
            .result
            .as_ref()
            .and_then(|result| result.pick.as_deref());
        let body = body
            .child(self.walkr_heading(fl!("walkr-detail-options").into()))
            .children(detail.options.iter().map(|option| {
                let comment = detail.result.as_ref().and_then(|result| {
                    result
                        .comments
                        .iter()
                        .find(|(id, _)| *id == option.id)
                        .map(|(_, comment)| comment.clone())
                });
                let body = match &option.body {
                    OptionBody::Text { text, cut } => div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().child(text.clone()))
                        .when(*cut, |body| {
                            body.child(self.walkr_muted(fl!("walkr-detail-cut")))
                        }),
                    OptionBody::NotDrawn { bytes } => {
                        self.walkr_muted(fl!("walkr-detail-html", size = bytes.div_ceil(1024)))
                    }
                };
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_3()
                    .rounded(px(crate::config::corners::CONTROL))
                    .border_1()
                    .border_color(rgb(theme.active))
                    .child(
                        div()
                            .row()
                            .gap_2()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(option.label.clone()),
                            )
                            .when(pick == Some(option.id.as_str()), |line| {
                                line.child(
                                    div()
                                        .text_color(rgb(theme.ink(theme.palette[2])))
                                        .child(fl!("walkr-detail-picked")),
                                )
                            }),
                    )
                    .children(
                        option
                            .summary
                            .clone()
                            .map(|summary| self.walkr_muted(summary)),
                    )
                    .child(body)
                    .children(comment.map(|comment| {
                        div()
                            .flex()
                            .flex_col()
                            .child(self.walkr_heading(fl!("walkr-detail-comment").into()))
                            .child(div().child(comment))
                    }))
            }));
        let body = body.when(detail.options_omitted > 0, |body| {
            body.child(self.walkr_muted(fl!(
                "walkr-detail-options-omitted",
                count = detail.options_omitted
            )))
        });
        let body = body.child(self.walkr_heading(fl!("walkr-detail-pick").into()));
        let body = match &detail.result {
            None => body.child(self.walkr_muted(fl!("walkr-detail-waiting"))),
            Some(result) => {
                let picked = result.pick.as_ref().map(|pick| {
                    detail
                        .options
                        .iter()
                        .find(|option| option.id == *pick)
                        .map_or_else(|| pick.clone(), |option| option.label.clone())
                });
                let body = match picked {
                    Some(label) => body.child(div().child(label)),
                    None => body.child(self.walkr_muted(fl!("walkr-detail-pick-none"))),
                };
                body.when(result.comments_omitted > 0, |body| {
                    body.child(self.walkr_muted(fl!(
                        "walkr-detail-comments-omitted",
                        count = result.comments_omitted
                    )))
                })
                .children(result.note.clone().map(|note| {
                    div()
                        .flex()
                        .flex_col()
                        .child(self.walkr_heading(fl!("walkr-detail-note").into()))
                        .child(div().child(note))
                }))
            }
        };
        let body = body.children(detail.outcome.clone().map(|outcome| {
            div()
                .flex()
                .flex_col()
                .child(self.walkr_heading(fl!("walkr-detail-outcome").into()))
                .child(div().child(outcome))
        }));
        body.when(!detail.ratings.is_empty(), |body| {
            body.child(self.walkr_heading(fl!("walkr-detail-ratings").into()))
                .children(detail.ratings.iter().rev().map(|rating| {
                    div()
                        .flex()
                        .flex_col()
                        .child(div().child(fl!("walkr-detail-rating", stars = stars(rating.value))))
                        .children(rating.note.clone().map(|note| self.walkr_muted(note)))
                }))
                .when(detail.ratings.len() < detail.ratings_total, |body| {
                    let enabled = !self.walkr.sessions.loading_detail();
                    body.child(
                        self.walkr_button(
                            "walkr-earlier-ratings",
                            fl!("walkr-earlier-ratings").into(),
                            enabled,
                        )
                        .when(enabled, |button| {
                            button.on_click(cx.listener(|this, _, _, cx| {
                                this.walkr.sessions.earlier_ratings();
                                cx.notify();
                            }))
                        }),
                    )
                })
        })
    }
}
