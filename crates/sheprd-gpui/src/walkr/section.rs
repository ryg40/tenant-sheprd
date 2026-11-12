//! Walkr's connection header and section-owned page.
// i18n: migrated
use super::{Connectivity, Unreachable, header::Label};
use crate::{SheprdWindow, config::corners, direction::Logical, i18n::fl};
use gpui::{prelude::*, *};

pub(super) fn can_open(state: Connectivity) -> bool {
    matches!(state, Connectivity::Reachable | Connectivity::Ready)
}

/// Whether the probe says that the server does not serve Walkr now.
pub(super) fn failing(state: Connectivity) -> bool {
    matches!(
        state,
        Connectivity::Unreachable { .. }
            | Connectivity::NetworkRefused
            | Connectivity::Down { .. }
            | Connectivity::Unexpected { .. }
    )
}

/// `host` is Walkr's host and port; another page's label holds its own host.
pub(super) fn title(label: &Label, host: &str) -> SharedString {
    let state = match label {
        Label::State(state) => *state,
        Label::OtherPage(other) => {
            return fl!("walkr-other-page-title", host = other.as_ref()).into();
        }
        Label::Live => return fl!("walkr-run-live-title", host = host).into(),
        Label::Ended => return fl!("walkr-run-ended-title", host = host).into(),
        Label::NoAnswer => return fl!("walkr-run-no-answer-title", host = host).into(),
    };
    match state {
        Connectivity::NotConfigured => fl!("walkr-not-configured-title"),
        Connectivity::Checking => fl!("walkr-checking-title"),
        Connectivity::Unreachable { cause } => match cause {
            Unreachable::Dns => fl!("walkr-dns-title"),
            Unreachable::Connect => fl!("walkr-connect-title"),
            Unreachable::Timeout => fl!("walkr-timeout-title"),
            Unreachable::Tls => fl!("walkr-tls-title"),
        },
        Connectivity::NetworkRefused => fl!("walkr-network-refused-title"),
        Connectivity::Down { .. } => fl!("walkr-down-title"),
        Connectivity::Unexpected { .. } => fl!("walkr-unexpected-title"),
        Connectivity::Reachable => fl!("walkr-reachable-title"),
        Connectivity::Ready => fl!("walkr-ready-title", host = host),
    }
    .into()
}

fn explanation(state: Connectivity, host: &str) -> SharedString {
    match state {
        Connectivity::NotConfigured => fl!("walkr-not-configured-detail"),
        Connectivity::Checking => fl!("walkr-checking-detail", host = host),
        Connectivity::Unreachable { cause } => match cause {
            Unreachable::Dns => fl!("walkr-dns-detail", host = host),
            Unreachable::Connect => fl!("walkr-connect-detail", host = host),
            Unreachable::Timeout => fl!("walkr-timeout-detail", host = host),
            Unreachable::Tls => fl!("walkr-tls-detail", host = host),
        },
        Connectivity::NetworkRefused => fl!("walkr-network-refused-detail"),
        Connectivity::Down { status } => fl!("walkr-down-detail", host = host, status = status),
        Connectivity::Unexpected { status } => {
            fl!("walkr-unexpected-detail", host = host, status = status)
        }
        Connectivity::Reachable => fl!("walkr-reachable-detail", host = host),
        Connectivity::Ready => fl!("walkr-ready-detail", host = host),
    }
    .into()
}

impl SheprdWindow {
    pub(crate) fn render_walkr_section(
        &self,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = &self.theme;
        let state = self.walkr.connectivity();
        let label = self.walkr.label();
        let run = self.walkr.run();
        // Nothing connected: a normal place that says how a run comes.
        let idle = run.is_none() && self.config.walkr.url.is_none();
        let host = run.map(super::run::Run::host);
        let host = host
            .or_else(|| {
                let url = self.config.walkr.url.as_ref()?;
                let address = url.address();
                let default_port = if url.as_str().starts_with("https:") {
                    ":443"
                } else {
                    ":80"
                };
                Some(
                    address
                        .strip_suffix(default_port)
                        .unwrap_or(&address)
                        .to_owned(),
                )
            })
            .unwrap_or_default();
        let color = match state {
            Connectivity::NotConfigured | Connectivity::Checking => theme.muted,
            Connectivity::Reachable | Connectivity::Ready => theme.ink(theme.palette[2]),
            _ => theme.ink(theme.palette[1]),
        };
        let refresh = self
            .walkr_button(
                "walkr-refresh",
                fl!("walkr-refresh").into(),
                !self.walkr.busy(),
            )
            .when(!self.walkr.busy(), |button| {
                button.on_click(cx.listener(|this, _, _, cx| {
                    if !this.walkr.busy() {
                        this.walkr.active(std::time::Instant::now());
                        this.walkr.refresh();
                        cx.notify();
                    }
                }))
            });
        #[cfg(embedded_pages)]
        let open_label = fl!("walkr-open-tab");
        #[cfg(not(embedded_pages))]
        let open_label = fl!("walkr-open-browser");
        let open = self
            .walkr_button("walkr-open", open_label.into(), can_open(state))
            .when(can_open(state), |button| {
                button.on_click(cx.listener(|this, _, window, cx| {
                    if !can_open(this.walkr.connectivity()) {
                        return;
                    }
                    let Some(url) = this.config.walkr.url.clone() else {
                        return;
                    };
                    #[cfg(embedded_pages)]
                    {
                        this.open_browser_tab(Some(url), window, cx);
                        this.show_activity_panel(
                            crate::sidebar::ActivityPanel::Workspaces,
                            window,
                            cx,
                        );
                    }
                    #[cfg(not(embedded_pages))]
                    {
                        let _ = window;
                        cx.open_url(url.as_str());
                    }
                }))
            });
        #[cfg(embedded_pages)]
        let reload = self
            .walkr_button(
                "walkr-reload",
                fl!("walkr-reload").into(),
                self.walkr_page_element().is_some(),
            )
            .on_click(cx.listener(|this, _, _, _| this.reload_walkr_page()));
        // The address of a run goes to no tab and no other program, so a
        // run has Close and Show page in place of an open button.
        #[cfg(embedded_pages)]
        let close = (run.is_some() && self.walkr.page_open()).then(|| {
            self.walkr_button("walkr-close", fl!("walkr-close").into(), true)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.dismiss_walkr_page();
                    cx.notify();
                }))
        });
        #[cfg(embedded_pages)]
        let show = self.walkr.can_show_page().then(|| {
            self.walkr_button("walkr-show-page", fl!("walkr-show-page").into(), true)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.show_walkr_page(window, cx);
                }))
        });
        let detail = match run {
            Some(_) if failing(state) => fl!("walkr-run-no-answer-detail").into(),
            #[cfg(embedded_pages)]
            Some(_) if self.walkr.can_show_page() => fl!("walkr-run-page-closed").into(),
            _ => explanation(state, &host),
        };
        let no_page = match run {
            Some(_) => fl!("walkr-run-no-page"),
            None => fl!("walkr-no-page"),
        };
        #[cfg(embedded_pages)]
        let page = self
            .walkr_page_element()
            .filter(|_| self.walkr.page_visible());
        #[cfg(not(embedded_pages))]
        let page: Option<AnyElement> = None;
        let body = match page {
            Some(page) => div()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .child(page)
                .into_any_element(),
            None => {
                let body = div()
                    .id("walkr-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .p_5()
                    .bg(rgb(theme.background))
                    .text_center()
                    .text_color(rgb(theme.muted))
                    .child(detail)
                    .when(idle, |body| body.child(fl!("walkr-sessions-detail")))
                    .when(self.walkr.error().is_some(), |body| {
                        body.child(fl!("walkr-worker-failed"))
                    })
                    .when(!crate::browser::EMBEDDED && !idle, |body| {
                        body.child(no_page)
                    });
                #[cfg(embedded_pages)]
                let body = body.when_some(self.walkr.page_error.clone(), |body, error| {
                    body.child(error).child(
                        self.walkr_button(
                            "walkr-retry",
                            fl!("walkr-retry").into(),
                            can_open(state),
                        )
                        .when(can_open(state), |button| {
                            button.on_click(cx.listener(|this, _, window, cx| {
                                let now = std::time::Instant::now();
                                this.walkr.active(now);
                                this.close_walkr_page();
                                this.poll_walkr_page(now, window, cx);
                                cx.notify();
                            }))
                        }),
                    )
                });
                body.into_any_element()
            }
        };
        let body = self.render_walkr_history(body, cx);
        // From a saved session back to the page of the run, while there is one.
        let back = self
            .walkr
            .detail_shown()
            .then_some(run)
            .flatten()
            .and_then(|run| {
                let label = if run.live() {
                    fl!("walkr-back-live")
                } else if self.walkr.page_open() {
                    fl!("walkr-back-page")
                } else {
                    return None;
                };
                Some(
                    self.walkr_button("walkr-back", label.into(), true)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.walkr.sessions.back();
                            #[cfg(embedded_pages)]
                            this.show_walkr_page(window, cx);
                            #[cfg(not(embedded_pages))]
                            let _ = window;
                            cx.notify();
                        })),
                )
            });
        div()
            .id("walkr-section")
            .debug_selector(|| "walkr-section".into())
            .flex()
            .flex_col()
            .flex_1()
            .size_full()
            .min_w_0()
            .min_h_0()
            // A native page lies beneath GPUI on macOS; leave its body transparent.
            .text_color(rgb(theme.foreground))
            .text_size(px(self.config.ui.size))
            .child(
                div()
                    .flex_none()
                    .bg(rgb(theme.background))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            #[cfg(embedded_pages)]
                            this.blur_walkr_page();
                            if let Some(focus) = &this.activity.section_focus {
                                window.focus(focus, cx);
                            }
                        }),
                    )
                    .px_5()
                    .py_3()
                    .row()
                    .flex_wrap()
                    .items_center()
                    .gap_3()
                    .border_b_1()
                    .border_color(rgb(theme.active))
                    .child(div().size(px(7.)).flex_none().rounded_full().bg(rgb(color)))
                    .child(self.walkr.header.render(&label, &host))
                    .when(!label.names_host(), |header| {
                        header.child(
                            div()
                                .min_w_0()
                                .text_color(rgb(theme.muted))
                                .child(host.clone()),
                        )
                    })
                    .child(div().flex_1())
                    .children(back)
                    .when(!idle, |header| header.child(refresh))
                    .map(|header| {
                        #[cfg(embedded_pages)]
                        let header = header.child(reload).children(show).children(close);
                        header
                    })
                    .when(run.is_none() && !idle, |header| header.child(open)),
            )
            .child(body)
            .into_any_element()
    }

    #[cfg(embedded_pages)]
    pub(super) fn show_walkr_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = std::time::Instant::now();
        self.walkr.show_page(now);
        self.poll_walkr_page(now, window, cx);
        cx.notify();
    }

    pub(super) fn walkr_button(
        &self,
        id: &'static str,
        label: SharedString,
        enabled: bool,
    ) -> Stateful<Div> {
        crate::accessibility::disabled(
            div()
                .id(id)
                .debug_selector(move || id.into())
                .role(Role::Button)
                .aria_label(label.clone())
                .px_3()
                .py_1()
                .row()
                .items_center()
                .rounded(px(corners::CONTROL))
                .border_1()
                .border_color(rgb(self.theme.active))
                .text_size(px(self.config.ui.size - 1.))
                .when(enabled, |button| {
                    button
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(self.theme.active)))
                })
                .when(!enabled, |button| button.opacity(0.45))
                .child(label),
            !enabled,
        )
    }
}
