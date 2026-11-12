use super::panel::{rows, toggle, window};
use crate::file_tree::search::DELAY;
use gpui::TestAppContext;
use std::fs;

/// Typing fast reads the checkout's index once, however often the field
/// changes while it is read, and the files listed are those of the text
/// typed last.
#[gpui::test]
fn fast_typing_reads_one_index_and_lists_the_last_query(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let init = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["init", "-q"])
        .output()
        .unwrap();
    assert!(init.status.success());
    fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("readme.txt"), "read me\n").unwrap();
    let (view, cx) = window(cx, &root);
    toggle(&view, cx);
    let builds = |cx: &mut gpui::VisualTestContext| {
        view.read_with(cx, |view, _| view.file_tree.searching.builds)
    };
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            for typed in ["r", "re", "rea"] {
                view.search_files(typed.into(), cx);
            }
            // Typing has not rested: nothing is read yet.
            assert_eq!(view.file_tree.searching.builds, 0);
            // The wait ends twice while the first read still runs.
            view.rank_files(false, cx);
            view.rank_files(false, cx);
            assert_eq!(view.file_tree.searching.builds, 1);
            view.search_files("main".into(), cx);
        });
    });
    cx.executor().advance_clock(DELAY);
    cx.run_until_parked();
    assert_eq!(builds(cx), 1);
    assert_eq!(rows(&view, cx), [("main.rs".to_owned(), false)]);
}
