use super::*;
use crate::walkr::section::can_open;
use crate::{
    browser::WebUrl,
    sidebar::{ActivityPanel, layout_tests::fixture_window},
};

#[test]
fn open_requires_a_reachable_state() {
    for state in [
        Connectivity::NotConfigured,
        Connectivity::Checking,
        Connectivity::Unreachable {
            cause: Unreachable::Dns,
        },
        Connectivity::Unreachable {
            cause: Unreachable::Connect,
        },
        Connectivity::Unreachable {
            cause: Unreachable::Timeout,
        },
        Connectivity::Unreachable {
            cause: Unreachable::Tls,
        },
        Connectivity::NetworkRefused,
        Connectivity::Down { status: 502 },
        Connectivity::Unexpected { status: 404 },
    ] {
        assert!(!can_open(state));
    }
    for state in [Connectivity::Reachable, Connectivity::Ready] {
        assert!(can_open(state));
    }
}

#[gpui::test]
fn probes_require_the_walkr_section_or_live_demand(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.config.sidebar_style.activity_bar = true;
        view.config.walkr.url = Some(WebUrl::try_from("https://walkr.example.test").unwrap());
        view
    });
    for (panel, demand, wanted) in [
        (ActivityPanel::Workspaces, None, false),
        (ActivityPanel::Walkr, None, true),
        (ActivityPanel::Usage, Some(DEMAND), true),
        (ActivityPanel::Workspaces, Some(Duration::ZERO), false),
    ] {
        let (worker, commands, _release) = Worker::fake();
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.walkr = Walkr {
                    worker: Some(worker),
                    ..Walkr::default()
                };
                view.show_activity_panel(panel, window, cx);
                if let Some(duration) = demand {
                    view.walkr.demand(Instant::now() + duration);
                }
                view.update_walkr();
                let worker = view.walkr.worker.as_ref().unwrap();
                // The wake channel also carries cancellation, so read its command mailbox.
                let _ = commands.try_recv();
                assert_eq!(worker.requested(), wanted);
            })
        });
    }
}
