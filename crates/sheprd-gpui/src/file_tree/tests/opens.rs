// Not a glob: the parent's `gpui::*` would shadow the `#[test]` that
// `gpui::test` expands to.
use super::panel::{toggle, window};
use crate::{
    browser::{
        Location, Store,
        file_view::{FileView, view_for},
    },
    control::SourceWhy,
};
use gpui::TestAppContext;
use std::{fs, path::Path};

/// A file opened from the tree gets the view every other way of opening it
/// gives: a page, the source of a file in a hidden folder with the reason
/// its bar shows, or code.
#[gpui::test]
fn a_file_opens_from_the_tree_in_the_view_the_file_view_gives_it(cx: &mut TestAppContext) {
    // Not the default name, which starts with a dot: a hidden folder.
    let dir = tempfile::Builder::new().prefix("tree").tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("docs/.drafts")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    for file in ["docs/guide.md", "docs/.drafts/plan.md", "src/main.rs"] {
        fs::write(root.join(file), "text\n").unwrap();
    }
    let (view, cx) = window(cx, &root);
    toggle(&view, cx);
    let home = crate::config::home().ok();
    let mut open = |file: &str| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.open_tree_file_as(file.to_owned(), true, window, cx);
            });
        });
        cx.run_until_parked();
        let path = root.join(file);
        let expected = view_for(&path, home.as_deref(), true).unwrap();
        let location = Location::from(expected.clone());
        let opened = cx.read(|cx| {
            cx.global::<Store>()
                .tabs()
                .any(|tab| tab.location.as_ref() == Some(&location))
        });
        assert!(opened, "{file} has no tab of its view");
        expected
    };
    assert!(matches!(open("docs/guide.md"), FileView::Page(_)));
    assert!(matches!(
        open("docs/.drafts/plan.md"),
        FileView::Source(file, SourceWhy::Folder) if Path::new(&file.path).ends_with("plan.md")
    ));
    assert!(matches!(open("src/main.rs"), FileView::Code(_)));
}
