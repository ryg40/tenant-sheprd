use super::*;
use crate::walkr::run::{self, Disconnected, Pane};
use sheprd_control_protocol::protocol::walkr::RunAddress;

/// What no log, reply, error, or `Debug` output may hold.
const SECRET_PARTS: [&str; 5] = [
    "INVENTEDSECRET",
    "_walkr",
    "compare",
    "20261101T120000Z",
    "0123456789ab",
];

fn address(host: &str) -> RunAddress {
    RunAddress::new(format!(
        "http://{host}/_walkr/compare/g/INVENTEDSECRET#20261101T120000Z-0123456789ab"
    ))
}

fn parsed(port: u16) -> run::Parsed {
    run::parse(&address(&format!("127.0.0.1:{port}"))).unwrap()
}

fn pane(id: &str) -> Pane {
    Pane::new(id, Some("/sandbox/a.sock".into()))
}

fn assert_no_secret(shown: &str) {
    for hidden in SECRET_PARTS {
        assert!(!shown.contains(hidden), "{shown}");
    }
}

#[test]
fn only_http_on_a_loopback_host_with_a_port_is_an_address() {
    for host in ["127.0.0.1:4100", "localhost:4100", "[::1]:4100"] {
        let parsed = run::parse(&address(host)).unwrap();
        assert_eq!(parsed.host(), host);
        assert_no_secret(&format!("{parsed:?}"));
    }
    for refused in [
        "https://127.0.0.1:4100/_walkr/compare/g/INVENTEDSECRET",
        "ftp://127.0.0.1:4100/INVENTEDSECRET",
        "http://192.168.1.20:4100/_walkr/compare/g/INVENTEDSECRET",
        "http://walkr.example.test:4100/INVENTEDSECRET",
        "http://localhost.walkr.example.test:4100/INVENTEDSECRET",
        "http://127.0.0.1.walkr.example.test:4100/INVENTEDSECRET",
        "http://user@127.0.0.1:4100/INVENTEDSECRET",
        "http://user:INVENTEDSECRET@localhost:4100/",
        "http://@127.0.0.1:4100/INVENTEDSECRET",
        "http://127.0.0.1:4100@walkr.example.test/INVENTEDSECRET",
        "http://127.0.0.1/INVENTEDSECRET",
        "http://127.0.0.1:/INVENTEDSECRET",
        "http://127.0.0.1:0/INVENTEDSECRET",
        "http://127.0.0.1:99999/INVENTEDSECRET",
        "http://127.0.0.1:4100/INVENTED SECRET",
        "127.0.0.1:4100/INVENTEDSECRET",
        "INVENTEDSECRET",
        "",
    ] {
        let error = run::parse(&RunAddress::new(refused)).unwrap_err();
        assert!(matches!(error, Error::InvalidAddress), "{refused}");
        assert_no_secret(&error.to_string());
    }
}

#[test]
fn an_address_over_the_bound_is_refused_and_one_at_the_bound_passes() {
    let base = "http://127.0.0.1:4100/";
    let at = format!("{base}{}", "a".repeat(RunAddress::MAX_BYTES - base.len()));
    assert!(run::parse(&RunAddress::new(at.clone())).is_ok());
    let error = run::parse(&RunAddress::new(format!("{at}a"))).unwrap_err();
    assert!(matches!(error, Error::AddressTooLong));
    let long = format!("{base}{}", "INVENTEDSECRET".repeat(215));
    assert_no_secret(&run::parse(&RunAddress::new(long)).unwrap_err().to_string());
}

#[test]
fn the_session_is_read_from_a_fragment_of_the_id_shape_only() {
    let now = Instant::now();
    for (fragment, session) in [
        (
            "#20261101T120000Z-0123456789ab",
            Some("20261101T120000Z-0123456789ab"),
        ),
        ("#20261101T120000Z-0123456789AB", None),
        ("#20261101T120000Z-0123456789abc", None),
        ("#2026110aT120000Z-0123456789ab", None),
        ("#other", None),
        ("", None),
    ] {
        let address = RunAddress::new(format!("http://127.0.0.1:4100/g/INVENTEDSECRET{fragment}"));
        let mut walkr = Walkr::default();
        walkr
            .connect(run::parse(&address).unwrap(), pane("p1"), now)
            .unwrap();
        assert_eq!(walkr.run().unwrap().session(), session);
    }
}

#[test]
fn debug_of_a_run_and_of_walkr_holds_host_and_port_only() {
    let mut walkr = Walkr::default();
    walkr
        .connect(parsed(4100), pane("p1"), Instant::now())
        .unwrap();
    for shown in [
        format!("{:?}", walkr.run().unwrap()),
        format!("{walkr:?}"),
        format!("{walkr:#?}"),
    ] {
        assert!(shown.contains("127.0.0.1:4100"), "{shown}");
        assert_no_secret(&shown);
    }
}

#[test]
fn a_run_has_priority_over_the_setting_as_the_probe_target() {
    let config = WalkrConfig {
        url: Some(crate::browser::WebUrl::try_from("https://walkr.example.test/app").unwrap()),
        probe_path: "/health".into(),
    };
    let mut walkr = Walkr::default();
    let now = Instant::now();
    walkr.poll(false, &config, now);
    let (url, path) = walkr.target().unwrap();
    assert_eq!(
        (url.host(), path.as_str()),
        ("walkr.example.test", "/health")
    );
    walkr.connect(parsed(4100), pane("p1"), now).unwrap();
    // The probe asks the origin, which holds no part of the path.
    let (url, path) = walkr.target().unwrap();
    assert_eq!(
        (url.as_str(), path.as_str()),
        ("http://127.0.0.1:4100/", "/sdk.js")
    );
    // Without a page the run goes with its last pane, and the setting is back.
    assert_eq!(walkr.disconnect(&pane("p1")), Disconnected::Dropped);
    assert_eq!(walkr.target().unwrap().0.host(), "walkr.example.test");
}

#[test]
fn a_probe_result_of_a_replaced_run_is_dropped() {
    let mut walkr = Walkr::default();
    let now = Instant::now();
    assert!(
        !walkr
            .connect(parsed(4100), pane("p1"), now)
            .unwrap()
            .replaced
    );
    let first = walkr.generation;
    // The same server from another pane keeps the target: a result asked
    // for before the pane joined still counts.
    assert!(
        !walkr
            .connect(parsed(4100), pane("p2"), now)
            .unwrap()
            .replaced
    );
    assert!(walkr.apply(Update {
        generation: first,
        reach: Reach::Answers,
        checked: now,
    }));
    assert_eq!(walkr.connectivity(), Connectivity::Reachable);
    assert!(
        walkr
            .connect(parsed(4200), pane("p1"), now)
            .unwrap()
            .replaced
    );
    assert_eq!(walkr.run().unwrap().host(), "127.0.0.1:4200");
    assert!(!walkr.apply(Update {
        generation: first,
        reach: Reach::Answers,
        checked: now,
    }));
    assert_eq!(walkr.connectivity(), Connectivity::Checking);
    assert!(walkr.apply(Update {
        generation: walkr.generation,
        reach: Reach::Answers,
        checked: now,
    }));
    assert_eq!(walkr.connectivity(), Connectivity::Reachable);
    // A result of a run that was forgotten does not count for the setting.
    let run = walkr.generation;
    walkr.disconnect(&pane("p1"));
    assert!(walkr.run().is_none());
    assert!(!walkr.apply(Update {
        generation: run,
        reach: Reach::Answers,
        checked: now,
    }));
}

#[test]
fn one_panes_disconnect_does_not_end_a_run_another_pane_holds() {
    let mut walkr = Walkr::default();
    let now = Instant::now();
    walkr.connect(parsed(4100), pane("p1"), now).unwrap();
    walkr.connect(parsed(4100), pane("p2"), now).unwrap();
    // The same pane of another daemon is another pane.
    let other = Pane::new("p1", Some("/sandbox/b.sock".into()));
    assert_eq!(walkr.disconnect(&other), Disconnected::NotHeld);
    assert_eq!(walkr.disconnect(&pane("p1")), Disconnected::StillLive);
    assert!(walkr.run().unwrap().live());
    assert_eq!(walkr.disconnect(&pane("p1")), Disconnected::NotHeld);
    assert!(walkr.run().unwrap().live());
    // No page is open here, so the run goes with its last pane.
    assert_eq!(walkr.disconnect(&pane("p2")), Disconnected::Dropped);
    assert!(walkr.run().is_none());
    assert_eq!(walkr.disconnect(&pane("p2")), Disconnected::NotHeld);
    let shown = format!("{walkr:?}");
    assert!(!shown.contains("4100"), "{shown}");
}

#[cfg(embedded_pages)]
#[test]
fn the_last_disconnect_gives_an_ended_run_that_keeps_its_address_and_page() {
    let mut walkr = Walkr::default();
    let now = Instant::now();
    walkr.connect(parsed(4100), pane("p1"), now).unwrap();
    let page = walkr.source(&WalkrConfig::default());
    assert!(page.is_some());
    walkr.page_for = page.clone();
    assert_eq!(walkr.disconnect(&pane("p1")), Disconnected::Ended);
    let run = walkr.run().unwrap();
    assert!(!run.live());
    assert_eq!(run.host(), "127.0.0.1:4100");
    assert_eq!(walkr.page_for, page);
    assert_eq!(walkr.label(), header::Label::Ended);
    // A connect to its server makes it live again, with the same page.
    let again = walkr.connect(parsed(4100), pane("p3"), now).unwrap();
    assert!(!again.replaced && !again.moved);
    assert!(walkr.run().unwrap().live());
    assert_eq!(walkr.page_for, page);
    assert_eq!(walkr.label(), header::Label::Live);
}

#[test]
fn a_full_run_refuses_one_more_pane_and_stays_as_it_was() {
    let mut walkr = Walkr::default();
    let start = Instant::now();
    let held = |index: usize| pane(&format!("p{index}"));
    for index in 0..run::MAX_PANES {
        walkr.connect(parsed(4100), held(index), start).unwrap();
    }
    let clock = walkr.last_activity;
    let later = start + Duration::from_secs(3600);
    // Another address of the same server, without a session.
    let moved = || {
        run::parse(&RunAddress::new(
            "http://127.0.0.1:4100/_walkr/compare/g/INVENTEDOTHER",
        ))
        .unwrap()
    };
    let error = walkr.connect(moved(), pane("one-more"), later).unwrap_err();
    assert!(matches!(error, Error::TooManyPanes));
    assert_no_secret(&error.to_string());
    // The address, the activity clock, and the panes are as before.
    assert!(walkr.run().unwrap().session().is_some());
    assert_eq!(walkr.last_activity, clock);
    assert_eq!(walkr.disconnect(&pane("one-more")), Disconnected::NotHeld);
    // A pane that holds the run still reconnects, with a newer address.
    assert!(walkr.connect(moved(), held(0), later).unwrap().moved);
    assert!(walkr.run().unwrap().session().is_none());
    assert_eq!(walkr.last_activity, later);
    // Every pane kept the run: each still lets go of it.
    for index in 1..run::MAX_PANES {
        assert_eq!(walkr.disconnect(&held(index)), Disconnected::StillLive);
    }
    assert_eq!(walkr.disconnect(&held(0)), Disconnected::Dropped);
}

#[test]
fn a_pane_its_daemon_lists_no_more_counts_as_its_disconnect() {
    let mut walkr = Walkr::default();
    let now = Instant::now();
    walkr.connect(parsed(4100), pane("p1"), now).unwrap();
    walkr.connect(parsed(4100), pane("p2"), now).unwrap();
    let listed = |boot: &'static str, panes: &'static [&'static str]| {
        move |pane: &Pane| Some((boot, panes.contains(&pane.id.as_str())))
    };
    // A pane the daemon never listed is not known to be gone.
    assert!(!walkr.observe_panes(listed("boot-1", &["p1"])));
    assert!(!walkr.observe_panes(listed("boot-1", &["p1", "p2"])));
    // Nor is one that a restarted daemon does not list, or with no snapshot.
    assert!(!walkr.observe_panes(listed("boot-2", &[])));
    assert!(!walkr.observe_panes(|_| None));
    assert!(!walkr.observe_panes(listed("boot-1", &["p1", "p2"])));
    assert!(walkr.observe_panes(listed("boot-1", &["p2"])));
    assert!(walkr.run().unwrap().live());
    assert_eq!(walkr.disconnect(&pane("p1")), Disconnected::NotHeld);
    assert!(walkr.observe_panes(listed("boot-1", &[])));
    assert!(walkr.run().is_none());
}

#[cfg(embedded_pages)]
#[test]
fn two_hours_without_activity_make_an_open_page_idle() {
    let second = Duration::from_secs(1);
    let hour = Duration::from_secs(3600);
    let mut walkr = Walkr::default();
    // The clock of a new section starts when it is made.
    let start = Instant::now();
    walkr.connect(parsed(4100), pane("p1"), start).unwrap();
    // No page, nothing to close.
    assert!(!walkr.idle(start + IDLE_CLOSE));
    walkr.page_for = walkr.source(&WalkrConfig::default());
    assert!(!walkr.idle(start + IDLE_CLOSE - second));
    assert!(walkr.idle(start + IDLE_CLOSE));
    // A connect for this run moves the limit.
    walkr
        .connect(parsed(4100), pane("p2"), start + hour)
        .unwrap();
    assert!(!walkr.idle(start + hour + IDLE_CLOSE - second));
    assert!(walkr.idle(start + hour + IDLE_CLOSE));
    // So does an input, a button, and a section that goes from hidden to shown.
    walkr.active(start + 2 * hour);
    assert!(!walkr.idle(start + 2 * hour + IDLE_CLOSE - second));
    let (worker, _commands, _release) = Worker::fake();
    walkr.worker = Some(worker);
    let config = WalkrConfig::default();
    walkr.poll(true, &config, start + 3 * hour);
    assert!(!walkr.idle(start + 3 * hour + IDLE_CLOSE - second));
    assert!(walkr.idle(start + 3 * hour + IDLE_CLOSE));
    // A section that stays shown, or stays hidden, is no activity.
    walkr.poll(true, &config, start + 4 * hour);
    walkr.poll(false, &config, start + 4 * hour);
    walkr.poll(false, &config, start + 4 * hour + second);
    assert!(walkr.idle(start + 3 * hour + IDLE_CLOSE));
    walkr.poll(true, &config, start + 4 * hour + second);
    assert!(!walkr.idle(start + 4 * hour + IDLE_CLOSE));
}

#[cfg(embedded_pages)]
#[test]
fn closing_the_page_keeps_a_live_run_and_forgets_an_ended_one() {
    let now = Instant::now();
    let mut walkr = Walkr::default();
    walkr.connect(parsed(4100), pane("p1"), now).unwrap();
    walkr.page_requested = true;
    walkr.page_for = walkr.source(&WalkrConfig::default());
    walkr.dismiss();
    assert!(walkr.run().unwrap().live());
    assert!(walkr.page_for.is_none() && walkr.can_show_page());
    assert_eq!(walkr.label(), header::Label::Live);
    walkr.test_reach(&WalkrConfig::default(), Some(Reach::Answers));
    assert!(!walkr.can_start_page(&WalkrConfig::default()));
    walkr.show_page(now);
    assert!(walkr.can_start_page(&WalkrConfig::default()));

    walkr.page_for = walkr.source(&WalkrConfig::default());
    assert_eq!(walkr.disconnect(&pane("p1")), Disconnected::Ended);
    walkr.dismiss();
    assert!(walkr.run().is_none());
    assert!(!walkr.can_show_page());
    let shown = format!("{walkr:?}");
    assert!(!shown.contains("4100"), "{shown}");
    assert_no_secret(&shown);
}

/// A build without pages: the header of the run, and no button that hands
/// its address to a tab or to another program.
#[cfg(not(embedded_pages))]
#[gpui::test]
fn a_build_without_pages_draws_a_run_with_no_open_button(cx: &mut gpui::TestAppContext) {
    use crate::sidebar::{
        ActivityPanel,
        layout_tests::{SidebarFixture, fixture_window, full_draw},
    };
    use gpui::{AppContext as _, px, size};
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut view = fixture_window(window, cx);
            view.config.sidebar_style.activity_bar = true;
            view
        });
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        SidebarFixture(view)
    });
    cx.simulate_resize(size(px(1100.), px(700.)));
    let view = cx.update(|_, cx| fixture.read(cx).0.clone());
    let redraw = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| full_draw(window, cx).clear(cx));
    };
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.connect_walkr(parsed(4100), pane("p1"), true, window, cx)
                .unwrap();
            let config = view.config.walkr.clone();
            view.walkr.test_reach(&config, Some(Reach::Answers));
            assert_eq!(view.activity_section(), Some(ActivityPanel::Walkr));
            assert_eq!(view.walkr.label(), header::Label::Live);
        });
    });
    redraw(cx);
    assert!(cx.debug_bounds("walkr-section").is_some());
    assert!(cx.debug_bounds("walkr-refresh").is_some());
    assert!(cx.debug_bounds("walkr-open").is_none());
    // The setting keeps its button once the run is gone.
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.config.walkr.url =
                Some(crate::browser::WebUrl::try_from("https://walkr.example.test").unwrap());
            assert_eq!(
                view.disconnect_walkr(&pane("p1"), cx),
                Disconnected::Dropped
            );
            let config = view.config.walkr.clone();
            view.walkr.test_reach(&config, Some(Reach::Answers));
        });
    });
    redraw(cx);
    assert!(cx.debug_bounds("walkr-open").is_some());
}
