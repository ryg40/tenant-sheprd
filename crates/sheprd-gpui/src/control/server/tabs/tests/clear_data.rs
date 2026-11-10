//! `browser clear-data` against a headless window.
use super::*;

/// The command only asks: the window shows the question, a second request
/// is refused while it is open, and the user's Cancel leaves everything.
#[cfg(servo_pages)]
#[gpui::test]
fn clearing_only_opens_the_question(cx: &mut gpui::TestAppContext) {
    use crate::menu::Page;
    let (view, cx) = window(cx);
    let caller = Caller::default();
    cx.cx.update(|cx| {
        assert!(matches!(
            page::clear_data(&caller, cx),
            Response::Done { .. }
        ));
    });
    assert!(view.read_with(cx, |view, _| view.menu.page == Some(Page::BrowserData)));
    cx.cx.update(|cx| {
        assert!(matches!(
            page::clear_data(&caller, cx),
            Response::Error {
                code: ErrorCode::WindowBusy,
                ..
            }
        ));
    });
    cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
    let cancel = cx.debug_bounds("browser-data-cancel").unwrap().center();
    cx.simulate_click(cancel, gpui::Modifiers::default());
    assert!(view.read_with(cx, |view, _| view.menu.page.is_none()));
    // A window that is not there is not asked, and neither is another
    // session's window for a caller whose workspace no window shows.
    let named = Caller {
        window: Some(u64::MAX),
        ..Caller::default()
    };
    let elsewhere = Caller {
        workspace_id: Some("w_missing".into()),
        ..Caller::default()
    };
    for (caller, refusal) in [
        (named, ErrorCode::NotFound),
        (elsewhere, ErrorCode::WorkspaceNotFound),
    ] {
        cx.cx.update(|cx| match page::clear_data(&caller, cx) {
            Response::Error { code, .. } => assert_eq!(code, refusal),
            _ => panic!("asked in a window that is not the caller's"),
        });
        assert!(view.read_with(cx, |view, _| view.menu.page.is_none()));
    }
}

/// A build whose pages keep their data elsewhere says that it cannot.
#[cfg(not(servo_pages))]
#[gpui::test]
fn clearing_is_refused_without_servo(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        assert!(matches!(
            page::clear_data(&Caller::default(), cx),
            Response::Error {
                code: ErrorCode::Unsupported,
                ..
            }
        ));
    });
}
