//! The file explorer's width is remembered with the rest of the chrome.
use super::*;

/// The width saved by an earlier run is the panel's width once the chrome
/// loads, so the next save of the chrome keeps it instead of erasing it.
#[gpui::test]
fn a_saved_file_tree_width_comes_back_with_the_chrome(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    view.update(cx, |view, _| {
        view.apply_stored_chrome(Ok(Chrome {
            file_tree_width: Some(360.),
            ..Chrome::default()
        }));
        assert_eq!(view.chrome().file_tree_width, Some(360.));
    });
}
