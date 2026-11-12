use super::*;
use crate::{
    browser::{TabId, reports::Event},
    sidebar::{ActivityPanel, layout_tests::SidebarFixture},
    walkr::{
        Connectivity, Disconnected,
        run::{self, Pane},
    },
};
use gpui::{AppContext as _, Context, Window, px, size};
use sheprd_control_protocol::protocol::walkr::RunAddress;
use std::time::Instant;

fn address(port: u16) -> String {
    format!("http://127.0.0.1:{port}/_walkr/compare/g/INVENTEDSECRET#20261101T120000Z-0123456789ab")
}

fn parsed(port: u16) -> run::Parsed {
    run::parse(&RunAddress::new(address(port))).unwrap()
}

fn pane(id: &str) -> Pane {
    Pane::new(id, None)
}

/// The fixture window at a size that draws its section, showing `w0`.
fn fixture(cx: &mut gpui::TestAppContext) -> (Entity<SheprdWindow>, &mut VisualTestContext) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut view = fixture_window(window, cx);
            view.config.sidebar_style.activity_bar = true;
            let mut shown = snapshot(40);
            shown.focused_workspace_id = Some("w0".into());
            shown.focused_tab_id = Some("t0".into());
            view.live.snapshot = Some(Arc::new(shown));
            view
        });
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        SidebarFixture(view)
    });
    cx.simulate_resize(size(px(1100.), px(700.)));
    let view = cx.update(|_, cx| fixture.read(cx).0.clone());
    (view, cx)
}

fn step<T>(
    view: &Entity<SheprdWindow>,
    cx: &mut VisualTestContext,
    act: impl FnOnce(&mut SheprdWindow, &mut Window, &mut Context<SheprdWindow>) -> T,
) -> T {
    cx.update(|window, cx| view.update(cx, |view, cx| act(view, window, cx)))
}

fn answers(view: &mut SheprdWindow) {
    view.walkr
        .test_connectivity(&view.config.walkr, Connectivity::Reachable);
}

fn tabs(cx: &gpui::App) -> usize {
    cx.try_global::<Store>()
        .map_or(0, |store| store.tabs().count())
}

#[gpui::test]
fn reports_of_a_runs_page_change_no_tab_and_its_own_server_opens_none(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = fixture(cx);
    step(&view, cx, |view, window, cx| {
        let connected = view
            .connect_walkr(parsed(4100), pane("p1"), true, window, cx)
            .unwrap();
        assert!(!connected.replaced);
        assert_eq!(view.activity_section(), Some(ActivityPanel::Walkr));
        answers(view);
        // As if the section's page was made for the run.
        view.walkr.page_for = view.walkr.source(&view.config.walkr);
        view.apply_page_event(Event::Loaded(TabId::WALKR, address(4100)), window, cx);
        assert_eq!(view.walkr.connectivity(), Connectivity::Ready);
        assert_eq!(tabs(cx), 0);
        for same_server in [
            address(4100),
            "http://127.0.0.1:4100/_walkr/compare/g/INVENTEDSECRET/next".to_owned(),
            "http://127.0.0.1:4100/".to_owned(),
        ] {
            view.apply_page_event(Event::NewWindow(TabId::WALKR, same_server), window, cx);
            assert_eq!(tabs(cx), 0);
        }
        // Another server is an ordinary link, and gets its tab.
        for other in ["http://127.0.0.1:4200/", "https://walkr.example.test/"] {
            view.apply_page_event(Event::NewWindow(TabId::WALKR, other.into()), window, cx);
        }
        assert_eq!(tabs(cx), 2);
        let store = cx.global::<Store>();
        assert!(store.get(TabId::WALKR).is_none());
        for tab in store.tabs() {
            let shown = format!("{:?} {:?}", tab.location, tab.title);
            assert!(
                !shown.contains("4100") && !shown.contains("INVENTED"),
                "{shown}"
            );
        }
    });
}

#[cfg(servo_pages)]
#[gpui::test]
fn the_page_of_a_run_waits_for_its_server_and_hides_while_it_does_not_answer(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = fixture(cx);
    step(&view, cx, |view, window, cx| {
        let now = Instant::now();
        view.connect_walkr(parsed(4100), pane("p1"), true, window, cx)
            .unwrap();
        // No page load starts by itself while the server does not answer.
        for state in [
            Connectivity::Checking,
            Connectivity::Unreachable {
                cause: crate::walkr::Unreachable::Connect,
            },
        ] {
            view.walkr.test_connectivity(&view.config.walkr, state);
            view.poll_walkr_page(now, window, cx);
            assert!(!view.browser.pages.contains(TabId::WALKR));
        }
        answers(view);
        view.poll_walkr_page(now, window, cx);
        assert!(view.browser.pages.contains(TabId::WALKR));
        assert!(view.walkr.page_visible());
        // The server stops: the page is hidden, not closed.
        view.walkr.test_connectivity(
            &view.config.walkr,
            Connectivity::Unreachable {
                cause: crate::walkr::Unreachable::Connect,
            },
        );
        view.poll_walkr_page(now, window, cx);
        assert!(view.browser.pages.contains(TabId::WALKR));
        assert!(!view.walkr.page_visible());
        answers(view);
        assert!(view.walkr.page_visible());
    });
}

#[cfg(servo_pages)]
#[gpui::test]
fn another_server_replaces_the_run_and_the_setting_returns_after_it(cx: &mut gpui::TestAppContext) {
    let (view, cx) = fixture(cx);
    step(&view, cx, |view, window, cx| {
        let now = Instant::now();
        view.config.walkr.url = Some(WebUrl::try_from("https://walkr.example.test").unwrap());
        view.show_activity_panel(ActivityPanel::Walkr, window, cx);
        answers(view);
        view.poll_walkr_page(now, window, cx);
        assert!(view.browser.pages.contains(TabId::WALKR));
        // A run has priority: the page of the setting closes.
        assert!(
            !view
                .connect_walkr(parsed(4100), pane("p1"), false, window, cx)
                .unwrap()
                .replaced
        );
        assert!(!view.browser.pages.contains(TabId::WALKR));
        answers(view);
        view.poll_walkr_page(now, window, cx);
        assert!(view.browser.pages.contains(TabId::WALKR));
        // The same server from another pane keeps the one page.
        assert!(
            !view
                .connect_walkr(parsed(4100), pane("p2"), false, window, cx)
                .unwrap()
                .replaced
        );
        assert!(view.browser.pages.contains(TabId::WALKR));
        // Another server replaces the run, and its page closes at once.
        assert!(
            view.connect_walkr(parsed(4200), pane("p2"), false, window, cx)
                .unwrap()
                .replaced
        );
        assert!(!view.browser.pages.contains(TabId::WALKR));
        assert_eq!(view.walkr.run().unwrap().host(), "127.0.0.1:4200");
        answers(view);
        view.poll_walkr_page(now, window, cx);
        assert!(view.browser.pages.contains(TabId::WALKR));
        // The run ends and its page is closed: the setting shows again.
        assert_eq!(view.disconnect_walkr(&pane("p2"), cx), Disconnected::Ended);
        assert!(view.browser.pages.contains(TabId::WALKR));
        view.dismiss_walkr_page();
        assert!(view.walkr.run().is_none());
        answers(view);
        view.poll_walkr_page(now, window, cx);
        assert!(view.browser.pages.contains(TabId::WALKR));
    });
}

#[cfg(servo_pages)]
#[gpui::test]
fn a_page_closes_after_two_hours_without_activity_shown_or_hidden(cx: &mut gpui::TestAppContext) {
    use crate::walkr::IDLE_CLOSE;
    let second = std::time::Duration::from_secs(1);
    let (view, cx) = fixture(cx);
    let start = step(&view, cx, |view, window, cx| {
        view.connect_walkr(parsed(4100), pane("p1"), true, window, cx)
            .unwrap();
        let start = Instant::now();
        answers(view);
        view.poll_walkr_page(start, window, cx);
        view.poll_walkr_page(start + IDLE_CLOSE - second, window, cx);
        assert!(view.browser.pages.contains(TabId::WALKR));
        // The clock runs for a hidden section too.
        view.show_activity_panel(ActivityPanel::Workspaces, window, cx);
        view.walkr.test_show(false, start);
        view.poll_walkr_page(start + IDLE_CLOSE, window, cx);
        assert!(!view.browser.pages.contains(TabId::WALKR));
        // A live run stays, and no page starts by itself.
        assert!(view.walkr.run().unwrap().live());
        view.show_activity_panel(ActivityPanel::Walkr, window, cx);
        view.walkr.test_show(true, start + IDLE_CLOSE);
        view.poll_walkr_page(start + IDLE_CLOSE, window, cx);
        assert!(!view.browser.pages.contains(TabId::WALKR));
        assert!(view.walkr.can_show_page());
        // A connect is activity, and loads the page again.
        let later = start + 3 * IDLE_CLOSE;
        view.walkr.connect(parsed(4100), pane("p1"), later).unwrap();
        view.poll_walkr_page(later, window, cx);
        assert!(view.browser.pages.contains(TabId::WALKR));
        view.poll_walkr_page(later + IDLE_CLOSE - second, window, cx);
        assert!(view.browser.pages.contains(TabId::WALKR));
        // So is a section that goes from hidden to shown.
        view.walkr.test_show(false, later);
        view.walkr.test_show(true, later + IDLE_CLOSE - second);
        view.poll_walkr_page(later + IDLE_CLOSE, window, cx);
        assert!(view.browser.pages.contains(TabId::WALKR));
        later + IDLE_CLOSE - second
    });
    // And an input that the window sends to the page.
    draw(cx);
    let section = cx.debug_bounds("walkr-section").unwrap();
    let over_page = gpui::point(section.center().x, section.bottom() - px(40.));
    cx.simulate_mouse_move(over_page, None, gpui::Modifiers::none());
    step(&view, cx, |view, window, cx| {
        let moved = start + IDLE_CLOSE - second;
        view.poll_walkr_page(moved, window, cx);
        view.poll_walkr_page(start + IDLE_CLOSE, window, cx);
        assert!(view.browser.pages.contains(TabId::WALKR));
        view.poll_walkr_page(moved + IDLE_CLOSE - second, window, cx);
        assert!(view.browser.pages.contains(TabId::WALKR));
        // The idle close of an ended run forgets the run.
        assert_eq!(view.disconnect_walkr(&pane("p1"), cx), Disconnected::Ended);
        view.poll_walkr_page(moved + IDLE_CLOSE, window, cx);
        assert!(!view.browser.pages.contains(TabId::WALKR));
        assert!(view.walkr.run().is_none());
        assert!(!view.activity_panel_listed(ActivityPanel::Walkr));
    });
}

#[cfg(servo_pages)]
#[gpui::test]
fn close_keeps_a_live_run_with_show_page_and_forgets_an_ended_one(cx: &mut gpui::TestAppContext) {
    fn redraw(cx: &mut VisualTestContext) {
        draw(cx);
    }
    fn press(cx: &mut VisualTestContext, button: &'static str) {
        redraw(cx);
        let bounds = cx.debug_bounds(button).unwrap();
        cx.simulate_click(bounds.center(), gpui::Modifiers::none());
    }
    let (view, cx) = fixture(cx);
    step(&view, cx, |view, window, cx| {
        view.connect_walkr(parsed(4100), pane("p1"), true, window, cx)
            .unwrap();
        answers(view);
        view.poll_walkr_page(Instant::now(), window, cx);
        assert!(view.browser.pages.contains(TabId::WALKR));
    });
    redraw(cx);
    // No button hands the address of a run to a tab or another program.
    assert!(cx.debug_bounds("walkr-open").is_none());
    assert!(cx.debug_bounds("walkr-show-page").is_none());

    press(cx, "walkr-close");
    step(&view, cx, |view, _, _| {
        assert!(!view.browser.pages.contains(TabId::WALKR));
        assert!(view.walkr.run().unwrap().live());
    });
    redraw(cx);
    assert!(cx.debug_bounds("walkr-close").is_none());

    press(cx, "walkr-show-page");
    step(&view, cx, |view, _, cx| {
        assert!(view.browser.pages.contains(TabId::WALKR));
        assert_eq!(view.disconnect_walkr(&pane("p1"), cx), Disconnected::Ended);
        assert!(view.browser.pages.contains(TabId::WALKR));
    });

    press(cx, "walkr-close");
    step(&view, cx, |view, _, _| {
        assert!(!view.browser.pages.contains(TabId::WALKR));
        assert!(view.walkr.run().is_none());
        let shown = format!("{:?}", view.walkr);
        assert!(
            !shown.contains("4100") && !shown.contains("INVENTED"),
            "{shown}"
        );
        assert!(!view.activity_panel_listed(ActivityPanel::Walkr));
    });
}
