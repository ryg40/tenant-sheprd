//! A page that Servo built for a new window, and the tab that holds it.
use super::*;
use crate::{
    browser::{Location, Store, TabId, scope},
    sidebar::layout_tests::{fixture_window, snapshot},
};
use std::sync::mpsc::{self, SyncSender};

/// What the engine keeps of the page, whose offer goes to `reports`.
fn open(reports: SyncSender<Event<Source>>) -> Open {
    Open {
        key: 1,
        url: String::new(),
        root: None,
        document: None,
        bridge: false,
        light: None,
        log_console: false,
        source: Source::Popup(1),
        reports,
        mailbox: Arc::default(),
        notifier: Arc::default(),
        latest: Arc::default(),
    }
}

/// A page that Servo built at `now` for `opener`, offered to the opener's
/// window as its first navigation goes to a web address.
fn offered(opener: Source, open: &Open, now: Instant) -> Opening {
    let opening = Opening::new(opener, now);
    let url = url::Url::parse("https://example.com/").unwrap();
    opening.navigates(&url, true, open, now);
    opening
}

/// Just after the time of a page that Servo built at `now` is up.
fn late(now: Instant) -> Instant {
    now + WAIT + Duration::from_millis(1)
}

#[test]
fn a_page_adopted_in_time_stays_and_one_no_tab_adopts_ends() {
    let (reports, _offers) = mpsc::sync_channel(2);
    let open = open(reports);
    let now = Instant::now();
    let adopted = offered(Source::Popup(0), &open, now);
    let unheld = offered(Source::Popup(0), &open, now);
    assert!(adopted.adopted(&open, now));
    assert!(!adopted.ended(&open, late(now)));
    assert!(unheld.ended(&open, late(now)));
}

/// Whether the store has a tab other than `opener`.
fn beside(opener: TabId, cx: &gpui::App) -> bool {
    cx.try_global::<Store>()
        .is_some_and(|store| store.tabs().any(|tab| tab.id != opener))
}

/// The window takes the page in a tab, and its word reaches the engine
/// after the page's time is up. The engine is the tests' inert one: no page
/// runs.
#[gpui::test]
fn an_adoption_past_the_deadline_is_refused_and_leaves_no_tab(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        let mut shown = snapshot(40);
        shown.focused_workspace_id = Some("w0".into());
        shown.focused_tab_id = Some("t0".into());
        view.live.snapshot = Some(Arc::new(shown));
        view
    });
    cx.update(|window, cx| {
        let scope = scope(&view.read(cx).endpoints[0]);
        let location = Location::Web {
            url: WebUrl::try_from("https://site.test/app").unwrap(),
        };
        let opener =
            Store::update(cx, |store| store.open(scope, "w0", Some(location), None)).unwrap();
        view.update(cx, |view, cx| {
            view.show_browser_tab(opener, window, cx);
            let (reports, offers) = mpsc::sync_channel(1);
            let open = open(reports);
            let now = Instant::now();
            let opening = offered(Source::Tab(opener), &open, now);
            let offer = offers.try_recv().unwrap().resolve(|source| match source {
                Source::Tab(id) => Some(id),
                _ => None,
            });
            view.apply_page_event(offer.unwrap(), window, cx);
            assert!(beside(opener, cx));
            assert!(!opening.adopted(&open, late(now)));
            assert!(opening.ended(&open, late(now)));
            view.poll_browser(window, cx);
            assert!(!beside(opener, cx));
        });
    });
}
