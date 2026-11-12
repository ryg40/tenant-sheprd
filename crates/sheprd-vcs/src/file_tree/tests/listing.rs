use super::*;

#[test]
fn a_path_outside_the_root_is_refused() {
    let (_dir, root, _) = fixture();
    for path in [
        "..",
        "../outside",
        "src/../../outside",
        "/etc",
        "src//main.rs",
        ".",
        ".git",
        "src/.git/objects",
        "a\u{202e}b",
        "a\nb",
    ] {
        assert!(
            matches!(
                list(&root, path, &not_cancelled()),
                Err(crate::Error::FileTree(Error::Outside))
            ),
            "{path:?} was not refused"
        );
    }
    let long = "a/".repeat(MAX_DEPTH + 1);
    assert!(names(long.trim_end_matches('/')).is_err());
    assert!(names(&"a".repeat(MAX_NAME_BYTES + 1)).is_err());
}

#[cfg(unix)]
#[test]
fn a_symlink_out_of_the_root_is_listed_and_never_followed() {
    let (_dir, root, outside) = fixture();
    std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("src/deep")).unwrap();
    let listing = list(&root, "", &not_cancelled()).unwrap();
    let link = listing
        .entries
        .iter()
        .find(|entry| entry.name == "link")
        .unwrap();
    assert_eq!(link.kind, Kind::Symlink);
    for path in ["link", "src/deep"] {
        assert!(matches!(
            list(&root, path, &not_cancelled()),
            Err(crate::Error::FileTree(Error::NotFolder))
        ));
    }
}

#[test]
fn folders_come_first_and_git_is_never_listed() {
    let (_dir, root, _) = fixture();
    fs::create_dir_all(root.join(".git/objects")).unwrap();
    fs::create_dir_all(root.join("Zeta")).unwrap();
    fs::write(root.join("alpha.txt"), "").unwrap();
    let listing = list(&root, "", &not_cancelled()).unwrap();
    let kinds: Vec<Kind> = listing.entries.iter().map(|entry| entry.kind).collect();
    assert!(kinds.is_sorted(), "a file is listed before a folder");
    assert!(names_of(&listing).contains(&"Zeta"));
    assert!(!names_of(&listing).contains(&".git"));
}

#[test]
fn a_huge_folder_stays_bounded_and_says_there_is_more() {
    let (_dir, root, _) = fixture();
    let big = root.join("big");
    fs::create_dir(&big).unwrap();
    for file in 0..MAX_ENTRIES + 50 {
        fs::write(big.join(format!("f{file}")), "").unwrap();
    }
    let listing = list(&root, "big", &not_cancelled()).unwrap();
    assert!(listing.entries.len() <= MAX_ENTRIES);
    assert!(listing.more);
}

#[test]
fn a_file_is_not_listed_as_a_folder() {
    let (_dir, root, _) = fixture();
    assert!(matches!(
        list(&root, "README.md", &not_cancelled()),
        Err(crate::Error::FileTree(Error::NotFolder))
    ));
    assert!(matches!(
        list(&root, "missing", &not_cancelled()),
        Err(crate::Error::FileTree(Error::NotFolder))
    ));
}

#[test]
fn ignored_entries_are_marked_and_still_listed() {
    let (_dir, root, _) = fixture();
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"]);
    fs::write(root.join(".gitignore"), "target/\n*.log\n").unwrap();
    fs::create_dir_all(root.join("target/debug")).unwrap();
    fs::write(root.join("target/debug/app"), "").unwrap();
    fs::write(root.join("build.log"), "").unwrap();
    let ignored = |listing: &Listing| -> Vec<String> {
        listing
            .entries
            .iter()
            .filter(|entry| entry.ignored)
            .map(|entry| entry.name.clone())
            .collect()
    };
    let top = list(&root, "", &not_cancelled()).unwrap();
    assert_eq!(ignored(&top), ["target", "build.log"]);
    assert!(names_of(&top).contains(&"README.md"));
    let inside = list(&root, "target", &not_cancelled()).unwrap();
    assert_eq!(ignored(&inside), ["debug"]);
}

#[test]
fn locate_refuses_a_path_outside_the_root() {
    let (_dir, root, outside) = fixture();
    assert_eq!(
        locate(&root, &root.join("src/main.rs")).unwrap(),
        "src/main.rs"
    );
    assert_eq!(locate(&root, &root).unwrap(), "");
    for path in [
        outside.join("secret.txt"),
        root.join("../outside/secret.txt"),
        root.join(".git/config"),
    ] {
        assert!(
            matches!(
                locate(&root, &path),
                Err(crate::Error::FileTree(Error::Outside))
            ),
            "{path:?} was located"
        );
    }
}

#[cfg(unix)]
#[test]
fn only_a_regular_file_inside_the_root_opens() {
    let (_dir, root, outside) = fixture();
    std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("link.md")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("out")).unwrap();
    assert_eq!(
        file(&root, "src/main.rs").unwrap(),
        root.join("src/main.rs")
    );
    for path in [
        "link.md",
        "out/secret.txt",
        "src",
        "",
        "../outside/secret.txt",
    ] {
        assert!(file(&root, path).is_err(), "{path:?} opened");
    }
}
