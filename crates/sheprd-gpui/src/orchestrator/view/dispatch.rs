//! Starting an agent on an item, and the run actions that change things:
//! the dispatch dialog, the confirmations for stopping an agent, removing a
//! worktree, or deleting a bead, and the banner with each action's outcome.

use super::OrchestratorView;
use crate::{
    config::{corners, mix},
    i18n::{Localize, fl},
    orchestrator::{
        Action, DispatchRequest, Item, Notice, SourceKey,
        actions::valid_model,
        prompt::{self, MAX_EXTRA},
        role::Role,
    },
    pull_request::MergeMethod,
    search_input::SearchInput,
    teleport::AgentKind,
};
use gpui::{prelude::*, *};
use sheprd_orchestrator::actions::Done;

mod dialog;
mod launch;
#[cfg(test)]
mod tests;

pub(crate) use launch::Last;

/// The dispatch dialog's choices.
pub(crate) struct Dialog {
    item: String,
    /// The prompt profile, by index into the snapshot's; `None` is built-in.
    profile: Option<usize>,
    kind: Option<AgentKind>,
    /// The role, by index into the config's; `None` is the agent's default.
    role: Option<usize>,
    branch: Entity<SearchInput>,
    extra: Entity<SearchInput>,
    /// The model the agent is started with; its own default when empty.
    model: Entity<SearchInput>,
    /// Another host to start on, by endpoint; the tab's own when `None`.
    pub(super) host: Option<String>,
    problem: Option<Problem>,
    /// A read-only review of a pull request rather than work on an issue.
    review: bool,
    /// The folded rows: prompt profile, model, extra instructions, host.
    more: launch::More,
}

impl Dialog {
    /// The canonical key of the item it dispatches.
    pub(super) fn item(&self) -> &str {
        &self.item
    }

    /// The focus handles of its fields.
    pub(super) fn focus_handles(&self, cx: &App) -> [FocusHandle; 3] {
        [&self.branch, &self.extra, &self.model].map(|field| field.read(cx).focus.clone())
    }

    /// Goes back to the agent's default role.
    pub(super) fn forget_role(&mut self) {
        self.role = None;
    }

    /// Picks `kind`, and goes back to the agent's default role when the
    /// chosen one does not fit it.
    fn set_kind(&mut self, kind: Option<AgentKind>, roles: &[Role]) {
        self.kind = kind;
        let fits = |index: usize| {
            roles
                .get(index)
                .zip(kind)
                .is_some_and(|(role, kind)| role.fits(kind))
        };
        self.role = self.role.filter(|&index| fits(index));
    }

    /// Picks the role `index`, or the agent's default for `None`, unless
    /// the chosen agent cannot take it. Says whether it took the choice.
    fn choose_role(&mut self, index: Option<usize>, roles: &[Role]) -> bool {
        let fits = index.is_none_or(|index| {
            roles
                .get(index)
                .zip(self.kind)
                .is_some_and(|(role, kind)| role.fits(kind))
        });
        if fits {
            self.role = index;
        }
        fits
    }

    /// The chosen role, when it fits the chosen agent.
    fn chosen_role<'a>(&self, roles: &'a [Role]) -> Option<&'a Role> {
        let kind = self.kind?;
        self.role
            .and_then(|index| roles.get(index))
            .filter(|role| role.fits(kind))
    }

    /// Moves focus from a folded field to the branch field, which always
    /// shows: keys go to the focused element's ancestors, so focus on a
    /// field that is not drawn would cut Enter and Escape off the dialog.
    fn keep_focus_shown(&self, window: &mut Window, cx: &mut App) {
        let folded = [&self.extra, &self.model]
            .into_iter()
            .any(|field| field.read(cx).focus.is_focused(window));
        if folded {
            window.focus(&self.branch.read(cx).focus.clone(), cx);
        }
    }

    /// Records a problem; one in a folded row opens the fold to show it.
    fn fail(&mut self, problem: Problem) {
        if matches!(problem, Problem::Extra | Problem::Model) && !self.more.toggle.is_open() {
            self.more.set(true);
        }
        self.problem = Some(problem);
    }
}

/// Why the dialog will not dispatch yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Problem {
    /// No agent installed on the chosen host is picked.
    Agent,
    /// Git would refuse the branch name.
    Branch,
    /// The extra instructions are over [`MAX_EXTRA`].
    Extra,
    /// The model name could become another option.
    Model,
}

impl Problem {
    fn text(self) -> String {
        match self {
            Self::Agent => fl!("orchestrator-problem-agent"),
            Self::Branch => fl!("orchestrator-problem-branch"),
            Self::Extra => fl!("orchestrator-problem-extra"),
            Self::Model => fl!("orchestrator-problem-model"),
        }
    }
}

/// A destructive action waiting for the user to confirm it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Confirm {
    /// Stopping run `run` of item `item`, which repository `repo`'s
    /// worker holds.
    Stop {
        run: String,
        agent: String,
        item: String,
        repo: String,
    },
    /// Removing run `run`'s worktree, bound like [`Confirm::Stop`].
    Remove {
        run: String,
        branch: String,
        item: String,
        repo: String,
    },
    DeleteBead {
        source: SourceKey,
        id: String,
    },
    /// Merging the pull request and head commit shown when it was asked.
    Merge {
        method: MergeMethod,
        target: crate::pr_actions::Target,
    },
}

/// What a finished action did, as the banner says it.
pub(super) fn done(done: Done) -> String {
    match done {
        Done::BeadDeleted => fl!("orchestrator-done-bead-deleted"),
        Done::Removed => fl!("orchestrator-done-removed"),
        Done::Sent => fl!("orchestrator-done-sent"),
        Done::Started => fl!("orchestrator-done-started"),
        Done::Stopped => fl!("orchestrator-done-stopped"),
    }
}

/// A merge method's confirming button, as GitHub words it.
pub(super) fn merge_action(method: MergeMethod) -> String {
    match method {
        MergeMethod::Merge => fl!("orchestrator-merge-commit"),
        MergeMethod::Squash => fl!("orchestrator-merge-squash"),
        MergeMethod::Rebase => fl!("orchestrator-merge-rebase"),
    }
}

impl Confirm {
    /// Its title, what it does, its button, and the button's ID.
    fn text(&self) -> (String, String, String, &'static str) {
        match self {
            Self::Stop { agent, .. } => (
                fl!("orchestrator-confirm-stop-title", agent = agent.as_str()),
                fl!("orchestrator-confirm-stop-body"),
                fl!("orchestrator-stop"),
                "stop",
            ),
            Self::Remove { branch, .. } if branch.is_empty() => (
                fl!("orchestrator-confirm-remove-run-title"),
                fl!("orchestrator-confirm-remove-run-body"),
                fl!("orchestrator-remove"),
                "remove",
            ),
            Self::Remove { branch, .. } => (
                fl!("orchestrator-confirm-remove-worktree-title"),
                fl!(
                    "orchestrator-confirm-remove-worktree-body",
                    branch = branch.as_str()
                ),
                fl!("orchestrator-remove"),
                "remove",
            ),
            Self::Merge { method, target } => (
                fl!(
                    "orchestrator-confirm-merge-title",
                    action = merge_action(*method),
                    number = target.number
                ),
                fl!(
                    "orchestrator-confirm-merge-body",
                    number = target.number,
                    head = target.head().get(..12).unwrap_or(target.head())
                ),
                fl!("orchestrator-merge"),
                "merge",
            ),
            Self::DeleteBead { id, .. } => (
                fl!("orchestrator-confirm-delete-bead-title", id = id.as_str()),
                fl!("orchestrator-confirm-delete-bead-body"),
                fl!("orchestrator-delete"),
                "delete",
            ),
        }
    }

    /// The worker action it confirms; a merge goes through `pr_actions`.
    fn action(self) -> Option<Action> {
        match self {
            Self::Stop { run, .. } => Some(Action::Stop { run }),
            Self::Remove { run, .. } => Some(Action::Remove { run }),
            Self::DeleteBead { source, id } => Some(Action::DeleteBead { source, id }),
            Self::Merge { .. } => None,
        }
    }
}

impl OrchestratorView {
    pub(crate) fn load_agents(
        &mut self,
        endpoint: Option<String>,
        target: sheprd_client::ConnectTarget,
        cx: &mut Context<Self>,
    ) {
        if !self
            .service()
            .is_some_and(|service| service.load_agents(endpoint, target))
        {
            self.report(crate::orchestrator::Error::Busy, cx);
        }
    }

    pub(super) fn select_agent_host(&mut self, cx: &mut Context<Self>) {
        if let Some(dialog) = &mut self.dialog {
            dialog.set_kind(None, &self.roles);
            if let Some(repo) = self.repos.get_mut(self.active) {
                repo.snapshot.installed = None;
            }
            if let Some(repo) = self.repos.get_mut(self.active) {
                repo.snapshot.installed_error = None;
            }
            cx.emit(super::Event::LoadAgents {
                endpoint: dialog.host.clone(),
            });
        }
    }

    pub(super) fn reconcile_agents(&mut self) {
        let Some(repo) = self.repos.get(self.active) else {
            return;
        };
        let snapshot = &repo.snapshot;
        if let Some(dialog) = &mut self.dialog
            && dialog.host == snapshot.installed_endpoint
            && let Some(installed) = &snapshot.installed
            && !dialog.kind.is_some_and(|kind| installed.contains(&kind))
        {
            // The last dialog's agent and role come back when the host has
            // that agent.
            let last = self
                .last_launch
                .kind
                .filter(|kind| installed.contains(kind));
            dialog.set_kind(last.or(installed.first().copied()), &self.roles);
            if last.is_some() {
                dialog.choose_role(self.last_launch.role, &self.roles);
            }
        }
    }

    /// Opens the dispatch dialog for the item with canonical key `item`.
    pub(super) fn open_dispatch(
        &mut self,
        item: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_dialog(item, false, window, cx);
    }

    /// Opens the dialog for a read-only review of the pull request `item`.
    pub(super) fn open_review(
        &mut self,
        item: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_dialog(item, true, window, cx);
    }

    fn open_dialog(
        &mut self,
        item: String,
        review: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.item_hidden(&item) {
            return;
        }
        self.activate(&item);
        if self.bound_hidden(&item) {
            return;
        }
        let Some(found) = self.item(&item) else {
            return;
        };
        let branch_name = match (&found.pull_request, review) {
            (Some(_), true) => crate::orchestrator::naming::review_branch(found),
            _ => crate::orchestrator::naming::branch(found),
        };
        let field = |placeholder: fn() -> String, text: &str, cx: &mut Context<Self>| {
            let (ui, theme) = (self.look.ui.clone(), self.look.theme.clone());
            cx.new(|cx| {
                let mut input = SearchInput::new(cx);
                input.set_placeholder_with(placeholder, cx);
                input.set_appearance(ui, theme, cx);
                if !text.is_empty() {
                    input.set_text_selected(text, cx);
                }
                input
            })
        };
        let branch = field(|| fl!("orchestrator-branch-placeholder"), &branch_name, cx);
        let extra = field(|| fl!("orchestrator-extra-placeholder"), "", cx);
        let model = field(|| fl!("orchestrator-model-placeholder"), "", cx);
        let kind = self
            .snapshot()
            .installed
            .as_ref()
            .and_then(|installed| installed.first().copied());
        // The branch field, as the folded fields are not drawn yet.
        let focus = branch.read(cx).focus.clone();
        self.dialog = Some(Dialog {
            item,
            profile: None,
            kind,
            role: None,
            branch,
            extra,
            model,
            host: None,
            problem: None,
            review,
            more: launch::More::default(),
        });
        self.select_agent_host(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(super) fn close_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let dialog = self.dialog.take();
        self.remember_launch(dialog.as_ref());
        let open = dialog.is_some() | self.confirm.take().is_some();
        if open {
            window.focus(&self.focus, cx);
            cx.notify();
        }
        open
    }

    fn submit_dispatch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A repository hidden since the dialog opened starts nothing.
        if self
            .dialog
            .as_ref()
            .is_some_and(|dialog| self.bound_hidden(&dialog.item))
        {
            self.close_overlays(window, cx);
            return;
        }
        // The dialog's item is in the active repository, which the dialog
        // made active when it opened.
        let (snapshot, workspace_id) = match self.repos.get(self.active) {
            Some(repo) => (&repo.snapshot, repo.request.workspace_id.clone()),
            None => (&self.blank, String::new()),
        };
        let Some(dialog) = &mut self.dialog else {
            return;
        };
        let Some(kind) = dialog.kind.filter(|kind| {
            dialog.host == snapshot.installed_endpoint
                && snapshot
                    .installed
                    .as_ref()
                    .is_some_and(|agents| agents.contains(kind))
        }) else {
            dialog.fail(Problem::Agent);
            cx.notify();
            return;
        };
        let branch = dialog.branch.read(cx).text().trim().to_owned();
        if !prompt::valid_branch(&branch) {
            dialog.fail(Problem::Branch);
            cx.notify();
            return;
        }
        let extra = dialog.extra.read(cx).text().to_owned();
        if extra.len() > MAX_EXTRA {
            dialog.fail(Problem::Extra);
            cx.notify();
            return;
        }
        let model = dialog.model.read(cx).text().trim().to_owned();
        if !model.is_empty() && !valid_model(&model) {
            dialog.fail(Problem::Model);
            cx.notify();
            return;
        }
        let host = dialog.host.clone();
        let Some(item) = snapshot
            .items
            .iter()
            .find(|item| item.key.canonical() == dialog.item)
        else {
            return;
        };
        let profile = dialog
            .profile
            .and_then(|index| snapshot.profiles.get(index));
        let remote = snapshot
            .repo
            .as_ref()
            .and_then(|info| info.remote.as_ref())
            .map(|remote| remote.url.as_str());
        let text = match (&item.pull_request, dialog.review) {
            (Some(pr), true) => prompt::compose_review(item, pr, remote, profile, &extra),
            _ => prompt::compose(item, profile, &extra),
        };
        let request = DispatchRequest {
            item_key: dialog.item.clone(),
            kind,
            model: (!model.is_empty()).then_some(model),
            prompt: text,
            title: crate::orchestrator::naming::title(item),
            branch,
            workspace_id,
            base: None,
            elsewhere: None,
            role: dialog
                .role
                .and_then(|index| self.roles.get(index))
                .filter(|role| role.fits(kind))
                .cloned(),
        };
        match host {
            // The window knows the other host and sets the request up for it.
            Some(endpoint) => cx.emit(super::Event::Dispatch {
                request: Box::new(request),
                endpoint,
            }),
            None => self.act(Action::Dispatch(Box::new(request))),
        }
        let dialog = self.dialog.take();
        self.remember_launch(dialog.as_ref());
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Keeps the agent and role of a closing dialog for the next one.
    fn remember_launch(&mut self, dialog: Option<&Dialog>) {
        if let Some(dialog) = dialog {
            self.last_launch = Last {
                kind: dialog.kind,
                role: dialog.role,
            };
        }
    }

    /// Hands `action` to the worker, or says why it could not take it.
    pub(super) fn act(&mut self, action: Action) {
        if self.kanban.shown {
            return;
        }
        #[cfg(test)]
        if let Some(sink) = &self.action_sink {
            let _ = sink.try_send(action);
            return;
        }
        let taken = self.service().is_some_and(|service| service.act(action));
        if !taken {
            self.notice = Some(Notice {
                outcome: Err(std::sync::Arc::new(crate::orchestrator::Error::Busy)),
            });
        }
    }

    /// Sends the message field's text to the newest run of the open item.
    pub(super) fn send_message(&mut self, cx: &mut Context<Self>) {
        let text = self.message.read(cx).text().trim().to_owned();
        // The worker the page's actions go to may no longer be shown.
        if self.open_item_hidden() {
            return;
        }
        let Some(detail) = &self.detail else {
            return;
        };
        let runs =
            super::rows::Runs::new(&self.snapshot().runs, &self.snapshot().sessions, &self.live);
        let Some(run) = runs
            .of_item(&detail.key)
            .first()
            .map(|row| runs.runs[row.run].id.clone())
        else {
            return;
        };
        if text.is_empty() {
            return;
        }
        self.act(Action::Send { run, text });
        self.message.update(cx, |input, cx| input.clear(cx));
    }

    pub(super) fn ask(&mut self, confirm: Confirm, cx: &mut Context<Self>) {
        self.confirm = Some(confirm);
        cx.notify();
    }

    /// The outcome of the newest action, with a dismiss button.
    pub(super) fn render_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let look = &self.look;
        let notice = self.notice.as_ref()?;
        let (hue, text) = match &notice.outcome {
            Ok(text) => (super::look::GREEN, done(*text)),
            Err(error) => (super::look::RED, error.localize()),
        };
        Some(
            div()
                .id("orchestrator-notice")
                .flex_none()
                .mx_4()
                .mb_2()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .rounded(px(corners::CONTROL))
                .bg(rgb(look.wash(hue)))
                .child(look.dot(look.hue(hue), 7.))
                .child(div().flex_1().min_w_0().text_size(look.small()).child(text))
                .child(
                    look.icon_button(
                        "orchestrator-notice-close",
                        "icons/close.svg",
                        fl!("orchestrator-dismiss"),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.notice = None;
                        cx.notify();
                    })),
                )
                .into_any_element(),
        )
    }

    /// The dialog or confirmation over the view, if one is open.
    pub(super) fn render_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let card = match (&self.dialog, &self.confirm) {
            (Some(dialog), _) => self.render_dialog(dialog, cx),
            (None, Some(confirm)) => self.render_confirm(confirm, cx),
            (None, None) => return None,
        };
        let theme = &self.look.theme;
        let backdrop = (mix(theme.background, theme.foreground, 12) << 8) | 0xB0;
        Some(
            div()
                .id("orchestrator-overlay")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .bg(rgba(backdrop))
                .flex()
                .items_center()
                .justify_center()
                .p_4()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                // A short window or large text scrolls the card rather than
                // pushing its fields and buttons out of reach.
                .child(
                    div()
                        .id("orchestrator-overlay-card")
                        .debug_selector(|| "orchestrator-overlay-card".into())
                        .max_h_full()
                        .max_w_full()
                        .overflow_y_scroll()
                        .child(card),
                )
                .into_any_element(),
        )
    }

    fn render_confirm(&self, confirm: &Confirm, cx: &mut Context<Self>) -> AnyElement {
        let look = &self.look;
        let (title, body, verb, key) = confirm.text();
        let confirm = confirm.clone();
        card(look)
            .w(px(420.))
            .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
            .child(
                div()
                    .text_size(look.small())
                    .text_color(rgb(look.theme.subtext()))
                    .child(body),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(div().flex_1())
                    .child(
                        look.button(
                            "orchestrator-confirm-cancel",
                            fl!("orchestrator-cancel"),
                            false,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_overlays(window, cx);
                        })),
                    )
                    .child(danger(look, key, verb).on_click(cx.listener(
                        move |this, _, window, cx| {
                            this.confirm_overlay(confirm.clone(), window, cx);
                        },
                    ))),
            )
            .into_any_element()
    }

    /// The confirming button of `confirm`, which it held since it was
    /// drawn. One whose repository the selection hides since, or whose
    /// worker is no longer the one shown, sends nothing.
    pub(super) fn confirm_overlay(
        &mut self,
        confirm: Confirm,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.confirm_hidden(&confirm) {
            match confirm {
                Confirm::Merge { method, target } => self.merge(method, &target, cx),
                confirm => {
                    if let Some(action) = confirm.action() {
                        self.act(action);
                    }
                }
            }
        }
        self.close_overlays(window, cx);
    }

    /// Keys while the dialog is open: Enter dispatches, Escape cancels.
    pub(super) fn dialog_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match (key, self.dialog.is_some(), self.confirm.is_some()) {
            ("escape", true, _) | ("escape", _, true) => self.close_overlays(window, cx),
            ("enter", true, _) => {
                self.submit_dispatch(window, cx);
                true
            }
            _ => false,
        }
    }
}

/// A profile's name in the dialog. Only a review has a built-in prompt a
/// profile may keep, so only there is replacing it worth saying.
fn profile_label(profile: &prompt::Profile, review: bool) -> String {
    match (review, profile.review) {
        (true, prompt::Review::Replace) => {
            fl!(
                "orchestrator-profile-replaces",
                name = profile.name.as_str()
            )
        }
        _ => profile.name.clone(),
    }
}

/// The strip holding a choice's [`segment`]s.
fn segments(look: &super::Look) -> Div {
    div()
        .flex()
        .flex_wrap()
        .p(px(2.))
        .gap(px(2.))
        .rounded(px(corners::CONTROL))
        .bg(rgb(look.theme.surface))
}

/// One option of a choice; `index` is `None` for its default, the first.
fn segment(
    look: &super::Look,
    choice: &str,
    index: Option<usize>,
    name: String,
    on: bool,
) -> Stateful<Div> {
    let theme = &look.theme;
    div()
        .id(SharedString::from(format!(
            "orchestrator-{choice}-{}",
            index.map_or(0, |index| index + 1)
        )))
        .px_3()
        .py(px(3.))
        .rounded(px(corners::SMALL))
        .cursor_pointer()
        .when(on, |el| el.bg(rgb(theme.background)))
        .text_size(look.small())
        .text_color(rgb(if on { theme.foreground } else { theme.muted }))
        .child(name)
}

fn card(look: &super::Look) -> Div {
    let theme = &look.theme;
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .rounded(px(corners::PANEL))
        .bg(rgb(theme.background))
        .border_1()
        .border_color(rgb(theme.active))
        .shadow_lg()
}

fn danger(look: &super::Look, key: &'static str, label: String) -> Stateful<Div> {
    let fill = look.hue(super::look::RED);
    div()
        .id(SharedString::from(format!("orchestrator-confirm-{key}")))
        .px_3()
        .py(px(4.))
        .rounded(px(corners::CONTROL))
        .bg(rgb(fill))
        .cursor_pointer()
        .text_color(rgb(look.theme.text_on(fill)))
        .text_size(look.small())
        .child(label)
}

fn item_line(look: &super::Look, item: &Item) -> Div {
    let theme = &look.theme;
    div()
        .flex()
        .items_center()
        .gap_2()
        .p_2()
        .rounded(px(corners::CONTROL))
        .bg(rgb(theme.surface))
        .child(super::list::source_mark(look, item.key.source.provider))
        .child(
            look.mono(item.identifier.clone())
                .text_color(rgb(theme.muted)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(item.title.clone()),
        )
}
