use super::*;

fn git(root: &Path, args: &[&str]) {
    let done = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(done.status.success(), "git {args:?}");
}

/// A repository's own configuration names another folder as its work tree.
/// The root stays the checkout the pane is in.
#[test]
fn the_root_does_not_follow_core_worktree() {
    let (dir, root, outside) = fixture();
    let root = root.canonicalize().unwrap();
    git(&root, &["init", "-q"]);
    let pane = root.join("src");
    let pane = pane.to_str().unwrap();
    assert_eq!(super::root(pane, &not_cancelled()).unwrap(), root);
    // Beside the checkout, and above it: neither holds this `.git`.
    for elsewhere in [outside.as_path(), dir.path()] {
        let elsewhere = elsewhere.canonicalize().unwrap();
        git(
            &root,
            &["config", "core.worktree", elsewhere.to_str().unwrap()],
        );
        assert_eq!(super::root(pane, &not_cancelled()).unwrap(), root);
    }
}
