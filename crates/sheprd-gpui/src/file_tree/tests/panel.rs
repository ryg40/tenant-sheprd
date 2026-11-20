// Not a glob: the parent's `gpui::*` would shadow the `#[test]` that
// `gpui::test` expands to.
use crate::{
    SheprdWindow,
    browser::{Location, Store},
    controls::Command,
    file_tree::Root,
};
use gpui::{Entity, TestAppContext, VisualTestContext};
use std::{fs, path::Path, sync::Arc};

/// A window whose focused pane works in `cwd`.
pub(super) fn window<'a>(
    cx: &'a mut TestAppContext,
    cwd: &Path,
) -> (Entity<SheprdWindow>, &'a mut VisualTestContext) {
    let cwd = cwd.to_owned();
    cx.add_window_view(move |window, cx| {
        let mut view = crate::sidebar::layout_tests::fixture_window(window, cx);
        focus_folder(&mut view, &cwd);
        view
    })
}

/// Makes the focused pane one that works in `cwd`.
pub(super) fn focus_folder(view: &mut SheprdWindow, cwd: &Path) {
    let mut shown = serde_json::to_value(crate::sidebar::layout_tests::snapshot(1)).unwrap();
    shown["focused_workspace_id"] = "w0".into();
    shown["focused_pane_id"] = "p0".into();
    shown["panes"] = serde_json::json!([{
        "pane_id": "p0", "workspace_id": "w0", "tab_id": "t0",
        "label": "shell", "cwd": cwd.to_str().unwrap(), "foreground_cwd": null,
        "focused": true, "right_click_passthrough": false
    }]);
    view.live.snapshot = Some(Arc::new(serde_json::from_value(shown).unwrap()));
}

pub(super) fn toggle(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.command(Command::ToggleFileExplorer, window, cx);
        });
    });
    cx.run_until_parked();
}

pub(super) fn rows(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext) -> Vec<(String, bool)> {
    view.read_with(cx, |view, _| {
        view.file_tree
            .shown()
            .into_iter()
            .map(|(path, folder)| (path.to_owned(), folder))
            .collect()
    })
}

pub(super) fn activate(view: &Entity<SheprdWindow>, path: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let row = view
                .file_tree
                .shown()
                .iter()
                .position(|(shown, _)| *shown == path)
                .unwrap();
            view.activate_file_row(row, window, cx);
        });
    });
    cx.run_until_parked();
}

/// The main path: the command shows the focused pane's folder, a folder
/// opens on demand, and a source file opens in a code tab.
#[gpui::test]
fn the_explorer_lists_the_focused_folder_and_opens_a_file(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("notes.txt"), "notes\n").unwrap();
    let (view, cx) = window(cx, &root);
    toggle(&view, cx);
    assert!(view.read_with(cx, |view, _| view.file_tree.is_open()));
    assert_eq!(
        view.read_with(cx, |view, _| view.file_tree.root().clone()),
        Root::Ready(root.clone())
    );
    let shown = rows(&view, cx);
    assert!(shown.contains(&("src".to_owned(), true)));
    assert!(shown.contains(&("notes.txt".to_owned(), false)));
    activate(&view, "src", cx);
    assert!(rows(&view, cx).contains(&("src/main.rs".to_owned(), false)));
    activate(&view, "src/main.rs", cx);
    let opened = root.join("src/main.rs");
    let code_tab = cx.read(|cx| {
        cx.global::<Store>().tabs().any(|tab| {
            matches!(&tab.location, Some(Location::Code { file }) if Path::new(&file.path) == opened)
        })
    });
    assert!(code_tab);
    toggle(&view, cx);
    assert!(!view.read_with(cx, |view, _| view.file_tree.is_open()));
}

/// A file outside the listed folder is not revealed.
#[gpui::test]
fn reveal_refuses_a_file_outside_the_root(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let root = base.join("root");
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(root.join("docs/guide.md"), "# Guide\n").unwrap();
    fs::write(base.join("secret.txt"), "secret\n").unwrap();
    let (view, cx) = window(cx, &root);
    toggle(&view, cx);
    let reveal = |path: std::path::PathBuf, cx: &mut VisualTestContext| {
        let revealed = cx.update(|_, cx| view.update(cx, |view, cx| view.reveal_file(&path, cx)));
        cx.run_until_parked();
        revealed
    };
    assert!(reveal(base.join("secret.txt"), cx).is_err());
    assert!(reveal(root.join("../secret.txt"), cx).is_err());
    assert_eq!(
        reveal(root.join("docs/guide.md"), cx).unwrap(),
        "docs/guide.md"
    );
    assert!(rows(&view, cx).contains(&("docs/guide.md".to_owned(), false)));
}
