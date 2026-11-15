//! The dialog's summary says what starts, and the role choice refuses a
//! role the chosen agent cannot take.
use super::*;
use crate::orchestrator::{
    role::Role,
    view::dispatch::launch::{Launch, unfit_reason},
};

fn planner() -> Role {
    Role {
        name: "planner".into(),
        agents: vec!["claude".into(), "pi".into()],
        prompt_file: "~/roles/planner.md".into(),
    }
}

fn launch(agent: AgentKind, role: Option<&Role>) -> Launch<'_> {
    Launch {
        agent: Some(agent),
        role,
        host: "host-a",
        model: "",
        branch: "issue-7-icons",
        ..Launch::default()
    }
}

fn says(lines: &[String], text: &str) -> bool {
    lines.iter().any(|line| line.contains(text))
}

#[test]
fn the_summary_names_no_role_when_none_is_chosen() {
    let lines = launch(AgentKind::Claude, None).summary();
    assert!(says(&lines, "Claude") && says(&lines, "host-a"));
    assert!(says(&lines, "issue-7-icons"));
    assert!(!says(&lines, "planner") && !says(&lines, "~/roles/planner.md"));
}

#[test]
fn one_role_reads_the_same_for_claude_and_pi() {
    let role = planner();
    for (kind, name) in [(AgentKind::Claude, "Claude"), (AgentKind::Pi, "Pi")] {
        let launch = launch(kind, Some(&role));
        let lines = launch.summary();
        assert!(says(&lines, name), "{lines:?}");
        assert!(says(&lines, "planner") && says(&lines, "~/roles/planner.md"));
        assert!(says(&lines, "host-a") && says(&lines, "issue-7-icons"));
        let button = launch.button();
        assert!(
            button.contains(name) && button.contains("planner"),
            "{button}"
        );
    }
}

#[test]
fn the_summary_names_each_folded_value_that_is_not_the_default() {
    use crate::orchestrator::prompt::Profile;
    let plain = launch(AgentKind::Pi, None).summary();
    let careful = Profile::parse("careful".into(), "Body");
    let changed = Launch {
        model: "big-model",
        profile: Some(&careful),
        extra: "Keep the icons small\nand the tests short",
        ..launch(AgentKind::Pi, None)
    }
    .summary();
    assert!(says(&changed, "big-model") && says(&changed, "careful"));
    // The extra instructions show as one line, not the hidden rest.
    assert!(says(&changed, "Keep the icons small"), "{changed:?}");
    assert!(!says(&changed, "the tests short"), "{changed:?}");
    assert!(!says(&plain, "careful") && !says(&plain, "Keep the icons"));
}

#[gpui::test]
fn a_role_the_agent_cannot_take_cannot_be_chosen(cx: &mut TestAppContext) {
    let roles = [planner()];
    assert!(unfit_reason(&roles[0], Some(AgentKind::Codex)).is_some());
    assert!(unfit_reason(&roles[0], None).is_some());
    assert!(unfit_reason(&roles[0], Some(AgentKind::Pi)).is_none());
    cx.update(|cx| {
        let field = cx.new(SearchInput::new);
        let mut dialog = Dialog {
            item: "issue".into(),
            profile: None,
            kind: Some(AgentKind::Codex),
            role: None,
            branch: field.clone(),
            extra: field.clone(),
            model: field,
            host: None,
            problem: None,
            review: false,
            more: Default::default(),
        };
        assert!(!dialog.choose_role(Some(0), &roles));
        assert_eq!(dialog.role, None);
        dialog.set_kind(Some(AgentKind::Claude), &roles);
        assert!(dialog.choose_role(Some(0), &roles));
        // Another agent the role serves keeps it.
        dialog.set_kind(Some(AgentKind::Pi), &roles);
        assert_eq!(dialog.role, Some(0));
        // One it does not serve goes back to the agent's default.
        dialog.set_kind(Some(AgentKind::Codex), &roles);
        assert_eq!(dialog.role, None);
        assert!(dialog.chosen_role(&roles).is_none());
    });
}
