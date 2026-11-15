//! The dispatch dialog's card. The agent and its role lead; the branch
//! stays in view; the prompt profile, model, extra instructions, and host
//! fold away beneath them. A summary above the start button says in words
//! what will start, folded values included.

use super::{Dialog, card, item_line, launch, profile_label, segment, segments};
use crate::{
    config::corners,
    i18n::fl,
    motion,
    orchestrator::{
        Provider,
        view::{OrchestratorView, look::agent_icon},
    },
    search_input::SearchInput,
    teleport::AgentKind,
};
use gpui::{prelude::*, *};

/// The most agent tiles shown.
const MAX_TILES: usize = 8;

impl OrchestratorView {
    pub(super) fn render_dialog(&self, dialog: &Dialog, cx: &mut Context<Self>) -> AnyElement {
        let look = &self.look;
        let item = self
            .snapshot()
            .items
            .iter()
            .find(|item| item.key.canonical() == dialog.item);
        let bead = item.is_some_and(|item| item.key.source.provider == Provider::Beads);
        let tab_host = match self
            .target()
            .cloned()
            .unwrap_or(sheprd_client::ConnectTarget::Local)
        {
            sheprd_client::ConnectTarget::Ssh { target, .. } => target.clone(),
            _ => fl!("orchestrator-this-machine"),
        };
        let host = dialog
            .host
            .as_ref()
            .map(|endpoint| {
                self.hosts
                    .iter()
                    .find(|candidate| &candidate.endpoint_id == endpoint)
                    .map_or_else(|| endpoint.clone(), |candidate| candidate.label.clone())
            })
            .unwrap_or(tab_host);
        let model = dialog.model.read(cx).text().trim().to_owned();
        let branch = dialog.branch.read(cx).text().trim().to_owned();
        let extra = dialog.extra.read(cx).text().to_owned();
        let launch = launch::Launch {
            agent: dialog.kind,
            role: dialog.chosen_role(&self.roles),
            host: &host,
            model: &model,
            branch: &branch,
            profile: dialog
                .profile
                .and_then(|index| self.snapshot().profiles.get(index)),
            extra: &extra,
            review: dialog.review,
        };
        let section = |title: String, body: AnyElement| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(look.label(title))
                .child(body)
        };
        let hosts = (!bead).then(|| self.render_hosts(dialog, cx)).flatten();
        let more = div()
            .flex()
            .flex_col()
            .gap_3()
            .pt_1()
            .child(section(
                fl!("orchestrator-section-prompt"),
                self.render_profiles(dialog, cx),
            ))
            .child(section(
                fl!("orchestrator-section-model"),
                self.field(&dialog.model).into_any_element(),
            ))
            .child(section(
                fl!("orchestrator-section-extra"),
                self.field(&dialog.extra).into_any_element(),
            ))
            // A bead's database stays on its own host, out of an agent's
            // reach on any other.
            .when_some(hosts, |el, hosts| {
                el.child(section(fl!("orchestrator-section-host"), hosts))
            });
        card(look)
            .w(px(600.))
            // The host the agent starts on, which More options may change.
            .child(self.render_dialog_head(dialog, host.clone(), cx))
            .when_some(item, |el, item| el.child(item_line(look, item)))
            .when(bead, |el| {
                el.child(
                    look.muted(fl!("orchestrator-claims-task"))
                        .debug_selector(|| "orchestrator-claims-task".into()),
                )
            })
            .child(section(
                fl!("orchestrator-section-agent"),
                self.render_agents(dialog, cx),
            ))
            .when_some(self.render_roles(dialog, cx), |el, roles| {
                el.child(section(fl!("orchestrator-section-role"), roles))
            })
            .child(section(
                fl!("orchestrator-section-branch"),
                self.field(&dialog.branch).into_any_element(),
            ))
            .child(self.render_more(dialog, more, cx))
            .when_some(dialog.problem, |el, problem| {
                el.child(
                    div()
                        .text_size(look.small())
                        .text_color(rgb(look.hue(super::super::look::RED)))
                        .child(problem.text()),
                )
            })
            .child(
                div()
                    .debug_selector(|| "orchestrator-launch-summary".into())
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_2()
                    .rounded(px(corners::CONTROL))
                    .bg(rgb(look.theme.surface))
                    .text_size(look.small())
                    .children(launch.summary()),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(div().flex_1())
                    .child(
                        look.button(
                            "orchestrator-dialog-cancel",
                            fl!("orchestrator-cancel"),
                            false,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_overlays(window, cx);
                        })),
                    )
                    .child(
                        look.button("orchestrator-dialog-dispatch", launch.button(), true)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.submit_dispatch(window, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_dialog_head(
        &self,
        dialog: &Dialog,
        host: String,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let look = &self.look;
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .text_size(px(look.ui.size + 3.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(if dialog.review {
                        fl!("orchestrator-review-with-agent")
                    } else {
                        fl!("orchestrator-dispatch-agent")
                    }),
            )
            .child(look.chip(fl!("orchestrator-on-host", host = host)))
            .child(
                look.icon_button(
                    "orchestrator-dialog-close",
                    "icons/close.svg",
                    fl!("orchestrator-close"),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_overlays(window, cx);
                })),
            )
    }

    fn field(&self, input: &Entity<SearchInput>) -> Div {
        let theme = &self.look.theme;
        div()
            .px_2()
            .py(px(4.))
            .rounded(px(corners::CONTROL))
            .bg(rgb(theme.background))
            .border_1()
            .border_color(rgb(theme.active))
            .child(input.clone())
    }

    fn render_profiles(&self, dialog: &Dialog, cx: &mut Context<Self>) -> AnyElement {
        let look = &self.look;
        let profiles = std::iter::once((None, fl!("orchestrator-profile-built-in"))).chain(
            self.snapshot()
                .profiles
                .iter()
                .enumerate()
                .map(|(index, profile)| (Some(index), profile_label(profile, dialog.review))),
        );
        segments(look)
            .children(profiles.map(|(index, name)| {
                segment(look, "profile", index, name, index == dialog.profile).on_click(
                    cx.listener(move |this, _, _, cx| {
                        if let Some(dialog) = &mut this.dialog {
                            dialog.profile = index;
                        }
                        cx.notify();
                    }),
                )
            }))
            .into_any_element()
    }

    /// The installed agents as tiles, those some role serves first, so
    /// the agents that share a role sit side by side.
    fn render_agents(&self, dialog: &Dialog, cx: &mut Context<Self>) -> AnyElement {
        let look = &self.look;
        let theme = &look.theme;
        let snapshot = self.snapshot();
        let mut agents: Vec<AgentKind> = snapshot
            .installed
            .as_ref()
            .filter(|_| dialog.host == snapshot.installed_endpoint)
            .map(|installed| installed.iter().copied().take(MAX_TILES).collect())
            .unwrap_or_default();
        if agents.is_empty() {
            return look
                .muted(
                    if dialog.host == snapshot.installed_endpoint
                        && snapshot.installed_error.is_some()
                    {
                        fl!("orchestrator-agents-failed")
                    } else if snapshot.installed.is_none()
                        || dialog.host != snapshot.installed_endpoint
                    {
                        fl!("orchestrator-agents-looking")
                    } else {
                        fl!("orchestrator-agents-none")
                    },
                )
                .into_any_element();
        }
        let served = |kind: &AgentKind| self.roles.iter().any(|role| role.fits(*kind));
        // A stable sort keeps the inventory's order within each group.
        agents.sort_by_key(|kind| !served(kind));
        div()
            .flex()
            .flex_wrap()
            .gap_2()
            .children(agents.into_iter().map(|kind| {
                let on = dialog.kind == Some(kind);
                div()
                    .id(SharedString::from(format!(
                        "orchestrator-agent-{}",
                        kind.name()
                    )))
                    .w(px(84.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_1()
                    .py_2()
                    .rounded(px(corners::CONTROL))
                    .border_1()
                    .border_color(rgb(if on { theme.primary() } else { theme.active }))
                    .when(on, |el| el.bg(rgb(theme.primary_wash())))
                    .cursor_pointer()
                    .child(look.icon(agent_icon(kind.name()), 20., theme.foreground))
                    .child(
                        div()
                            .text_size(look.small())
                            .child(launch::agent_name(kind)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(dialog) = &mut this.dialog {
                            dialog.set_kind(Some(kind), &this.roles);
                        }
                        cx.notify();
                    }))
            }))
            .into_any_element()
    }

    /// "Agent default" and each configured role, with chips for the agents
    /// it serves. A role the chosen agent cannot take shows why, dimmed;
    /// nothing when the config has no role.
    fn render_roles(&self, dialog: &Dialog, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.roles.is_empty() {
            return None;
        }
        let look = &self.look;
        let theme = &look.theme;
        let option = |id: Option<usize>, on: bool, enabled: bool| {
            div()
                .id(SharedString::from(format!(
                    "orchestrator-role-{}",
                    id.map_or(0, |index| index + 1)
                )))
                .flex()
                .flex_col()
                .gap_1()
                .min_w(px(140.))
                .p_2()
                .rounded(px(corners::CONTROL))
                .border_1()
                .border_color(rgb(if on { theme.primary() } else { theme.active }))
                .when(on, |el| el.bg(rgb(theme.primary_wash())))
                .when(enabled, |el| el.cursor_pointer())
                .when(!enabled, |el| el.opacity(0.55))
        };
        let pick = |index: Option<usize>| {
            cx.listener(move |this, _, _, cx| {
                let roles = &this.roles;
                if let Some(dialog) = &mut this.dialog {
                    dialog.choose_role(index, roles);
                }
                cx.notify();
            })
        };
        let default = option(None, dialog.role.is_none(), true)
            .child(div().child(fl!("orchestrator-role-default")))
            .child(look.muted(fl!("orchestrator-launch-role-default-note")))
            .on_click(pick(None));
        let roles = self.roles.iter().enumerate().map(|(index, role)| {
            let reason = launch::unfit_reason(role, dialog.kind);
            let on = dialog.role == Some(index) && reason.is_none();
            let chips = launch::role_agents(role).map(|kind| {
                let mine = dialog.kind == Some(kind);
                look.badge_in(
                    launch::agent_name(kind),
                    if mine {
                        theme.primary()
                    } else {
                        theme.subtext()
                    },
                )
            });
            let enabled = reason.is_none();
            option(Some(index), on, enabled)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(div().flex_1().min_w_0().truncate().child(role.name.clone()))
                        .children(chips),
                )
                .child(
                    look.mono(role.prompt_file.clone())
                        .text_color(rgb(theme.muted)),
                )
                .when_some(reason, |el, reason| el.child(look.muted(reason)))
                .when(enabled, |el| el.on_click(pick(Some(index))))
        });
        Some(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(default)
                .children(roles)
                .into_any_element(),
        )
    }

    /// The disclosure of the folded rows, and the rows while they show.
    /// They grow open and shrink closed over [`motion::ENTER`] and
    /// [`motion::LEAVE`], reversing from where they are.
    fn render_more(&self, dialog: &Dialog, rows: Div, cx: &mut Context<Self>) -> impl IntoElement {
        let look = &self.look;
        let theme = &look.theme;
        let now = motion::now();
        let toggle = dialog.more.toggle;
        let (open, shown, moving) = (toggle.is_open(), toggle.shown(now), toggle.moving(now));
        let measured = dialog.more.height.clone();
        let height = measured.get();
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("orchestrator-launch-more")
                    .flex()
                    .items_center()
                    .gap_1()
                    .cursor_pointer()
                    .child(look.icon(
                        if open {
                            "icons/chevron-down.svg"
                        } else {
                            "icons/chevron-right.svg"
                        },
                        12.,
                        theme.muted,
                    ))
                    .child(look.label(fl!("orchestrator-launch-more")))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(dialog) = &mut this.dialog {
                            let open = dialog.more.toggle.is_open();
                            dialog.more.set(!open);
                            if open {
                                dialog.keep_focus_shown(window, cx);
                            }
                        }
                        cx.notify();
                    })),
            )
            .when(shown > 0., |el| {
                el.child(
                    div()
                        .flex_none()
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .on_children_prepainted(move |bounds, window, _| {
                            if let Some(content) = bounds.first() {
                                measured.set(content.size.height);
                            }
                            if moving {
                                window.request_animation_frame();
                            }
                        })
                        .when(moving, |el| el.h(height * shown).opacity(shown))
                        .child(rows.flex_none()),
                )
            })
    }
}
