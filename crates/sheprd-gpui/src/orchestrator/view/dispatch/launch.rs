//! What the dispatch dialog will start, in words: the summary above its
//! start button and the button's own label, the choices a window keeps for
//! its next dialog, and the folded section of rarely changed rows.

use crate::{
    i18n::fl,
    motion,
    orchestrator::{prompt, role::Role},
    teleport::AgentKind,
};
use gpui::Pixels;
use std::{cell::Cell, rc::Rc};

/// Everything a dispatch starts with, as the dialog holds it now.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Launch<'a> {
    pub(super) agent: Option<AgentKind>,
    /// The chosen role, only when it fits `agent`.
    pub(super) role: Option<&'a Role>,
    pub(super) host: &'a str,
    /// The model name; the agent's own default when empty.
    pub(super) model: &'a str,
    pub(super) branch: &'a str,
    /// The chosen prompt profile; the built-in prompt when `None`.
    pub(super) profile: Option<&'a prompt::Profile>,
    /// The extra instructions, as typed.
    pub(super) extra: &'a str,
    pub(super) review: bool,
}

impl Launch<'_> {
    /// The launch as sentences: what starts where, then each folded value
    /// that is not its default, so nothing hidden changes it silently.
    pub(super) fn summary(&self) -> Vec<String> {
        let Some(kind) = self.agent else {
            return vec![fl!("orchestrator-launch-summary-no-agent")];
        };
        let agent = agent_name(kind);
        let model = if self.model.is_empty() {
            fl!("orchestrator-launch-model-default")
        } else {
            self.model.to_owned()
        };
        let (host, branch) = (self.host, self.branch);
        let first = match self.role {
            Some(role) => fl!(
                "orchestrator-launch-summary-role",
                agent = agent.as_str(),
                role = role.name.as_str(),
                file = role.prompt_file.as_str(),
                host = host,
                model = model.as_str(),
                branch = branch
            ),
            None => fl!(
                "orchestrator-launch-summary-default",
                agent = agent.as_str(),
                host = host,
                model = model.as_str(),
                branch = branch
            ),
        };
        let profile = self.profile.map(|profile| {
            let name = profile.name.as_str();
            match (self.review, profile.review) {
                // Out of view, the warning the profile row gave must stay.
                (true, prompt::Review::Replace) => {
                    fl!("orchestrator-launch-summary-profile-replaces", name = name)
                }
                _ => fl!("orchestrator-launch-summary-profile", name = name),
            }
        });
        let extra = excerpt(self.extra)
            .map(|text| fl!("orchestrator-launch-summary-extra", text = text.as_str()));
        std::iter::once(first).chain(profile).chain(extra).collect()
    }

    /// The start button's label, naming the agent and the role.
    pub(super) fn button(&self) -> String {
        let Some(kind) = self.agent else {
            return fl!("orchestrator-launch-start-pick");
        };
        let agent = agent_name(kind);
        match (self.review, self.role) {
            (false, None) => fl!("orchestrator-launch-start", agent = agent.as_str()),
            (false, Some(role)) => fl!(
                "orchestrator-launch-start-role",
                agent = agent.as_str(),
                role = role.name.as_str()
            ),
            (true, None) => fl!("orchestrator-launch-review", agent = agent.as_str()),
            (true, Some(role)) => fl!(
                "orchestrator-launch-review-role",
                agent = agent.as_str(),
                role = role.name.as_str()
            ),
        }
    }
}

/// The most characters of the extra instructions the summary quotes.
const EXCERPT: usize = 60;

/// The first line of `text`, cut at [`EXCERPT`] characters, with an
/// ellipsis when anything was left out; `None` when it is blank.
fn excerpt(text: &str) -> Option<String> {
    let text = text.trim();
    let line = text.lines().next()?.trim_end();
    let mut shown: String = line.chars().take(EXCERPT).collect();
    if shown.len() < text.len() {
        shown.push('…');
    }
    Some(shown)
}

/// An agent's product name, from the name Herdr reports: `claude` is
/// Claude. Product names are not translated.
pub(super) fn agent_name(kind: AgentKind) -> String {
    let name = kind.name();
    let mut chars = name.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Why `role` cannot be chosen for `kind`, or `None` when it can.
pub(super) fn unfit_reason(role: &Role, kind: Option<AgentKind>) -> Option<String> {
    let Some(kind) = kind else {
        return Some(fl!("orchestrator-launch-role-no-agent"));
    };
    if role.fits(kind) {
        return None;
    }
    let agent = agent_name(kind);
    Some(match kind.system_prompt_flag() {
        None => fl!("orchestrator-launch-role-no-option", agent = agent.as_str()),
        Some(_) => fl!("orchestrator-launch-role-not-for", agent = agent.as_str()),
    })
}

/// The agents a role serves, as its chips show them.
pub(super) fn role_agents(role: &Role) -> impl Iterator<Item = AgentKind> + '_ {
    role.agents
        .iter()
        .filter_map(|name| AgentKind::parse(name))
        .filter(|&kind| role.fits(kind))
}

/// The agent and role of a window's last dialog, which its next one
/// starts from. The role is an index into the config's roles, so a change
/// of roles forgets it, as it does an open dialog's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Last {
    pub(super) kind: Option<AgentKind>,
    pub(super) role: Option<usize>,
}

impl Last {
    pub(crate) fn forget_role(&mut self) {
        self.role = None;
    }
}

/// The folded section of the rows a launch rarely changes.
#[derive(Clone, Debug, Default)]
pub(super) struct More {
    pub(super) toggle: motion::Toggle,
    /// The section's height as last laid out, which the motion grows to.
    pub(super) height: Rc<Cell<Pixels>>,
}

impl More {
    /// Opens or closes it, with motion.
    pub(super) fn set(&mut self, open: bool) {
        self.toggle.set(open, motion::now(), false);
    }
}
