use super::panel::{activate, rows, toggle, window};
use gpui::TestAppContext;
use sheprd_vcs::file_tree::{Entry, Kind, Listing};
use std::fs;

fn listing(name: &str) -> Listing {
    Listing {
        entries: vec![Entry {
            name: name.to_owned(),
            kind: Kind::File,
            ignored: false,
        }],
        omitted: Default::default(),
    }
}

/// Two refreshes read one folder, and the read asked for first answers
/// last: the tree keeps what the newer read found.
#[gpui::test]
fn the_newer_of_two_reads_of_a_folder_stays(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    let (view, cx) = window(cx, &root);
    toggle(&view, cx);
    activate(&view, "src", cx);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let older = view.file_tree.reads.ask("src");
            let newer = view.file_tree.reads.ask("src");
            view.folder_listed("src".into(), newer, Ok(listing("new.rs")), cx);
            view.folder_listed("src".into(), older, Ok(listing("old.rs")), cx);
        });
    });
    let shown = rows(&view, cx);
    assert!(shown.contains(&("src/new.rs".to_owned(), false)));
    assert!(!shown.contains(&("src/old.rs".to_owned(), false)));
}
