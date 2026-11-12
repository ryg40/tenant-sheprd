//! A local file's tab between its page and its source.
use super::*;
use crate::{
    browser::{LocalFile, Shown, TabId},
    control::{Opening, Placed, Placement, Target},
};

/// Opens `path` as a request does, in the window's workspace, shown and
/// with the keyboard; its tab.
fn open(
    view: &Entity<SheprdWindow>,
    cx: &mut VisualTestContext,
    path: &str,
    source: bool,
) -> TabId {
    request(view, cx, path, source, true);
    view.read_with(cx, |view, cx| {
        let group = view.active_group().unwrap();
        match view.group_shown(group, cx) {
            Shown::Page(id) => id,
            other => panic!("the group shows {other:?}"),
        }
    })
}

fn request(
    view: &Entity<SheprdWindow>,
    cx: &mut VisualTestContext,
    path: &str,
    source: bool,
    focus: bool,
) {
    let file = LocalFile::new(std::path::Path::new(path), None).unwrap();
    let placed = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let target = Target {
                daemon: None,
                workspace: None,
                pane: Some("w0:p1"),
            };
            view.open_requested_browser_tab(
                &target,
                true,
                &Opening {
                    location: &Location::Local { file },
                    focus,
                    placement: Placement::Tab,
                    source,
                },
                window,
                cx,
            )
        })
    });
    assert!(matches!(placed, Some(Placed::Opened { .. })));
}

/// Asks for tab `id`'s rendered page, on a build that shows pages or not.
fn rendered(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext, id: TabId, embedded: bool) {
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.file_shown_as(id, false, true, embedded, window, cx);
        });
    });
}

#[gpui::test]
fn a_file_asked_for_as_source_shows_its_code_and_switches_back(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    let id = open(&view, cx, "/srv/site/plan.md", true);
    view.read_with(cx, |view, cx| {
        assert!(view.shows_file_source(id, cx));
        let file = view.code_file(id, cx).unwrap();
        assert_eq!(file.path, "/srv/site/plan.md");
    });
    rendered(&view, cx, id, true);
    view.read_with(cx, |view, cx| {
        assert!(!view.shows_file_source(id, cx));
        assert!(view.code_file(id, cx).is_none());
    });
    // The same file asked for again as source reuses the tab, as source.
    assert_eq!(open(&view, cx, "/srv/site/plan.md", true), id);
    view.read_with(cx, |view, cx| assert!(view.shows_file_source(id, cx)));
}

#[gpui::test]
fn a_source_tab_that_goes_to_a_web_address_shows_the_page(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    let id = open(&view, cx, "/srv/site/plan.html", true);
    // As the address field and `tab navigate` do.
    cx.update(|_, cx| {
        Store::update(cx, |store| store.asked(id, url("https://example.com/")));
    });
    view.read_with(cx, |view, cx| {
        assert!(!view.shows_file_source(id, cx));
        assert!(view.code_file(id, cx).is_none());
    });
}

#[gpui::test]
fn a_source_asked_for_without_focus_leaves_the_keyboard(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    let terminal = view.read_with(cx, |view, _| view.focus.clone());
    cx.update(|window, cx| window.focus(&terminal, cx));
    request(&view, cx, "/srv/site/plan.md", true, false);
    cx.update(|window, _| assert!(terminal.is_focused(window)));
}

#[gpui::test]
fn a_build_without_pages_refuses_the_rendered_page(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    let id = open(&view, cx, "/srv/site/plan.md", true);
    rendered(&view, cx, id, false);
    view.read_with(cx, |view, cx| assert!(view.shows_file_source(id, cx)));
    // A code tab of a page file stays a code tab there.
    let code = code_tab(&view, cx, "/srv/site/notes.md");
    rendered(&view, cx, code, false);
    assert!(is_code_tab(cx, code));
}

#[gpui::test]
fn a_page_opens_rendered_and_a_code_tab_of_a_page_file_becomes_its_page(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = window(cx);
    let id = open(&view, cx, "/srv/site/index.html", false);
    view.read_with(cx, |view, cx| assert!(!view.shows_file_source(id, cx)));
    // A code tab of a Markdown file switches to the file's page, served
    // from the file's own folder.
    let code = code_tab(&view, cx, "/srv/site/notes.md");
    rendered(&view, cx, code, true);
    view.read_with(cx, |_, cx| {
        let tab = cx.global::<Store>().get(code).unwrap();
        let Some(Location::Local { file }) = &tab.location else {
            panic!("still {:?}", tab.location);
        };
        assert_eq!(file.path(), std::path::Path::new("/srv/site/notes.md"));
    });
    // A file that is no page keeps its code tab, and so does a page file in
    // a hidden folder, which the preview scheme never serves.
    for path in ["/srv/site/main.rs", "/srv/.secrets/notes.md"] {
        let code = code_tab(&view, cx, path);
        rendered(&view, cx, code, true);
        assert!(is_code_tab(cx, code), "{path}");
    }
}

/// Opens `path` in a code tab; its tab.
fn code_tab(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext, path: &str) -> TabId {
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let target = crate::editor::EditorTarget {
                path: path.into(),
                line: None,
            };
            view.open_code_view(&target, window, cx);
            let group = view.active_group().unwrap();
            match view.group_shown(group, cx) {
                Shown::Page(code) => code,
                other => panic!("the group shows {other:?}"),
            }
        })
    })
}

fn is_code_tab(cx: &mut VisualTestContext, id: TabId) -> bool {
    cx.update(|_, cx| {
        matches!(
            cx.global::<Store>().get(id).unwrap().location,
            Some(Location::Code { .. })
        )
    })
}
