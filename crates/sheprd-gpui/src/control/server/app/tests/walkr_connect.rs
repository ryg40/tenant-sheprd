use super::*;
use crate::{
    control::{
        protocol::walkr::{Reply as WalkrReply, RunAddress, Source, Status},
        server::walkr::{Asked, connect, disconnect, source, status},
    },
    sidebar::{ActivityPanel, layout_tests::fixture_window},
    walkr::Connectivity,
};

const ADDRESS: &str =
    "http://127.0.0.1:4100/_walkr/compare/g/INVENTEDSECRET#20261101T120000Z-0123456789ab";
const SECRET_PARTS: [&str; 4] = ["INVENTEDSECRET", "_walkr", "compare", "20261101"];

fn from_pane(pane: &str) -> Caller {
    Caller {
        pane_id: Some(pane.into()),
        ..Caller::default()
    }
}

fn fixture(
    cx: &mut gpui::TestAppContext,
) -> (gpui::Entity<SheprdWindow>, &mut gpui::VisualTestContext) {
    cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.config.sidebar_style.activity_bar = true;
        view.activity.panel = ActivityPanel::Usage;
        view
    })
}

fn assert_no_secret(response: &Response) {
    let wire = serde_json::to_string(response).unwrap();
    for hidden in SECRET_PARTS {
        assert!(!wire.contains(hidden), "{wire}");
    }
    let shown = format!("{response:?}");
    for hidden in SECRET_PARTS {
        assert!(!shown.contains(hidden), "{shown}");
    }
}

#[gpui::test]
fn connect_answers_with_host_and_port_and_shows_the_section_when_asked(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = fixture(cx);
    let address = RunAddress::new(ADDRESS);
    let connected = cx
        .cx
        .update(|cx| connect(&from_pane("p1"), &address, false, cx));
    assert_no_secret(&connected);
    assert!(matches!(
        &connected,
        Response::Walkr {
            reply: WalkrReply::Connected { address, replaced: false }
        } if address == "127.0.0.1:4100"
    ));
    view.read_with(cx, |view, _| {
        assert!(view.activity_panel_listed(ActivityPanel::Walkr));
        assert_eq!(view.activity.panel, ActivityPanel::Usage);
    });
    // The status handler, once the probe of the run answered.
    let answered = view.update(cx, |view, _| {
        let config = view.config.walkr.clone();
        view.walkr
            .test_connectivity(&config, Connectivity::Reachable);
        view.walkr.sessions.test_listed(Vec::new());
        status(view, std::time::Instant::now(), &mut Asked::default())
    });
    let answered = answered.unwrap();
    assert_no_secret(&answered);
    assert!(matches!(
        &answered,
        Response::Walkr {
            reply: WalkrReply::Status(Status {
                address,
                source: Source::Run,
                live: true,
                ..
            })
        } if address == "127.0.0.1:4100"
    ));
    // Another server replaces the run, and `--show` shows the section.
    let other = RunAddress::new(ADDRESS.replace("4100", "4200"));
    let replaced = cx
        .cx
        .update(|cx| connect(&from_pane("p2"), &other, true, cx));
    assert!(matches!(
        &replaced,
        Response::Walkr {
            reply: WalkrReply::Connected { address, replaced: true }
        } if address == "127.0.0.1:4200"
    ));
    view.read_with(cx, |view, _| {
        assert_eq!(view.activity.panel, ActivityPanel::Walkr);
        assert_eq!(view.walkr.run().unwrap().host(), "127.0.0.1:4200");
    });
}

#[gpui::test]
fn a_refused_connect_changes_nothing_and_names_no_part_of_the_address(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = fixture(cx);
    let long = format!("{ADDRESS}{}", "a".repeat(RunAddress::MAX_BYTES));
    for refused in [
        ADDRESS.replace("http:", "https:"),
        ADDRESS.replace("127.0.0.1", "192.168.1.20"),
        ADDRESS.replace("127.0.0.1", "walkr.example.test"),
        ADDRESS.replace("127.0.0.1", "user@127.0.0.1"),
        ADDRESS.replace(":4100", ""),
        long,
    ] {
        let address = RunAddress::new(refused);
        let response = cx
            .cx
            .update(|cx| connect(&from_pane("p1"), &address, true, cx));
        assert_eq!(code(&response), Some(ErrorCode::InvalidUrl));
        assert_no_secret(&response);
    }
    let address = RunAddress::new(ADDRESS);
    // Outside a pane, as `browser reload`.
    let no_pane = cx
        .cx
        .update(|cx| connect(&Caller::default(), &address, false, cx));
    assert_eq!(code(&no_pane), Some(ErrorCode::InvalidRequest));
    let no_pane = cx.cx.update(|cx| disconnect(&Caller::default(), cx));
    assert_eq!(code(&no_pane), Some(ErrorCode::InvalidRequest));
    // A section that cannot show is not connected behind the caller's back.
    view.update(cx, |view, _| view.config.sidebar_style.activity_bar = false);
    let hidden = cx
        .cx
        .update(|cx| connect(&from_pane("p1"), &address, true, cx));
    assert_eq!(code(&hidden), Some(ErrorCode::Unsupported));
    view.read_with(cx, |view, _| {
        assert!(view.walkr.run().is_none());
        assert!(source(&view.walkr, &view.config.walkr).is_none());
        assert_eq!(view.activity.panel, ActivityPanel::Usage);
    });
}

#[gpui::test]
fn disconnect_is_idempotent_and_one_pane_does_not_end_the_run_of_another(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = fixture(cx);
    let done = |response: &Response| matches!(response, Response::Done { .. });
    // Nothing is connected: still done.
    assert!(done(&cx.cx.update(|cx| disconnect(&from_pane("p1"), cx))));
    let address = RunAddress::new(ADDRESS);
    for pane in ["p1", "p2"] {
        cx.cx
            .update(|cx| connect(&from_pane(pane), &address, false, cx));
    }
    assert!(done(&cx.cx.update(|cx| disconnect(&from_pane("p1"), cx))));
    view.read_with(cx, |view, _| {
        assert_eq!(
            source(&view.walkr, &view.config.walkr).map(|(from, live, _)| (from, live)),
            Some((Source::Run, true))
        );
    });
    // A second disconnect of the same pane changes nothing.
    assert!(done(&cx.cx.update(|cx| disconnect(&from_pane("p1"), cx))));
    view.read_with(cx, |view, _| assert!(view.walkr.run().unwrap().live()));
    assert!(done(&cx.cx.update(|cx| disconnect(&from_pane("p2"), cx))));
    // No page was made in this test, so the run is forgotten at once.
    view.read_with(cx, |view, _| assert!(view.walkr.run().is_none()));
}
