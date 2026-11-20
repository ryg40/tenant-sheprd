use super::panel::{activate, focus_folder, rows, toggle, window};
use crate::{SheprdWindow, file_tree::Root};
use gpui::{Entity, TestAppContext, VisualTestContext};
use std::fs;

/// What the window tick does: the tree follows the focused pane.
fn poll(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| view.poll_file_tree(cx));
    cx.run_until_parked();
}

fn root(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext) -> Root {
    view.read_with(cx, |view, _| view.file_tree.root().clone())
}

/// The tree follows the focus to another checkout, lists nothing of this
/// machine for a workspace on another host, and opens the folders that
/// were open in a checkout again when the focus comes back to it.
#[gpui::test]
fn a_remote_workspace_lists_nothing_and_a_checkout_keeps_its_open_folders(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let (first, second) = (base.join("first"), base.join("second"));
    fs::create_dir_all(first.join("src")).unwrap();
    fs::write(first.join("src/lib.rs"), "").unwrap();
    fs::create_dir_all(&second).unwrap();
    fs::write(second.join("notes.txt"), "").unwrap();
    let (view, cx) = window(cx, &first);
    toggle(&view, cx);
    activate(&view, "src", cx);
    assert!(rows(&view, cx).contains(&("src/lib.rs".to_owned(), false)));

    view.update(cx, |view, _| focus_folder(view, &second));
    poll(&view, cx);
    assert_eq!(root(&view, cx), Root::Ready(second.clone()));
    let shown = rows(&view, cx);
    assert!(shown.contains(&("notes.txt".to_owned(), false)));
    assert!(!shown.iter().any(|(path, _)| path.starts_with("src")));

    let local = view.update(cx, |view, _| {
        let endpoint = &mut view.endpoints[view.selected_endpoint].connection;
        std::mem::replace(
            &mut endpoint.target,
            sheprd_client::ConnectTarget::Ssh {
                target: "unused".into(),
                session: "default".into(),
            },
        )
    });
    poll(&view, cx);
    assert_eq!(root(&view, cx), Root::Remote);
    // No folder is read or asked for: the panel shows why in their place.
    assert!(view.read_with(cx, |view, _| view.file_tree.folders.is_empty()));

    view.update(cx, |view, _| {
        view.endpoints[view.selected_endpoint].connection.target = local;
        focus_folder(view, &first);
    });
    poll(&view, cx);
    assert_eq!(root(&view, cx), Root::Ready(first.clone()));
    assert!(rows(&view, cx).contains(&("src/lib.rs".to_owned(), false)));
}
