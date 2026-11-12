use super::*;

/// What the tree lists, it opens and finds: a colon and a backslash are
/// plain characters of a name where the platform takes them as such.
#[cfg(unix)]
#[test]
fn a_name_with_a_colon_or_a_backslash_lists_opens_and_is_found() {
    let (_dir, root, _) = fixture();
    for folder in ["build:2026", "back\\slash"] {
        fs::create_dir(root.join(folder)).unwrap();
        fs::write(root.join(folder).join("notes:10:00.md"), "# Notes").unwrap();
        let top = list(&root, "", &not_cancelled()).unwrap();
        assert!(names_of(&top).contains(&folder), "{folder:?} is not listed");
        let inside = list(&root, folder, &not_cancelled()).unwrap();
        assert_eq!(names_of(&inside), ["notes:10:00.md"]);
        let path = format!("{folder}/notes:10:00.md");
        let opened = file(&root, &path).unwrap();
        assert_eq!(opened, root.join(folder).join("notes:10:00.md"));
        assert_eq!(locate(&root, &opened).unwrap(), path);
    }
}

/// Windows separates names with a backslash and names drives and streams
/// with a colon, so no path may hold either there.
#[cfg(windows)]
#[test]
fn a_backslash_or_a_colon_in_a_name_is_refused() {
    for path in ["src\\..\\..", "a:b", "C:", "notes.md:stream"] {
        assert!(names(path).is_err(), "{path:?} was not refused");
    }
}
