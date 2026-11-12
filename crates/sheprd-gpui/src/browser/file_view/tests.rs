#![allow(clippy::unwrap_used)]
use super::{FileView, FileViews, Stamp, browser_view, crumbs, renders, view_for};
use crate::{
    browser::{Location, TabId},
    control::SourceWhy,
};
use std::path::{Path, PathBuf};

fn stamp(len: u64) -> Option<Stamp> {
    Some(Stamp {
        modified: None,
        len,
    })
}

#[test]
fn markdown_and_html_files_render_and_others_do_not() {
    for name in ["a.md", "b.MARKDOWN", "c.html", "d.HTM"] {
        assert!(renders(Path::new(name)), "{name}");
    }
    for name in ["a.rs", "html", "a.html.txt", "a.svg", "Makefile"] {
        assert!(!renders(Path::new(name)), "{name}");
    }
}

#[test]
fn a_view_reloads_only_when_its_own_file_changes() {
    let mut views = FileViews::default();
    let (tab, path) = (TabId::test(1), PathBuf::from("/site/a.md"));
    // First seen: the view just loaded it.
    assert!(views.changed([(tab, path.clone(), stamp(1))]).is_empty());
    assert!(views.changed([(tab, path.clone(), stamp(1))]).is_empty());
    assert_eq!(views.changed([(tab, path.clone(), stamp(2))]), [tab]);
    // A file that is gone for a moment, as while it is written again,
    // keeps the old page; its return with other content reloads.
    assert!(views.changed([(tab, path.clone(), None)]).is_empty());
    assert_eq!(views.changed([(tab, path, stamp(3))]), [tab]);
    // The tab went to another file, which its page loaded itself.
    let other = PathBuf::from("/site/b.md");
    assert!(views.changed([(tab, other, stamp(9))]).is_empty());
}

#[test]
fn a_breadcrumb_starts_at_the_workspace_folder_and_cleans_names() {
    let root = Path::new("/home/u/stacks");
    let home = Path::new("/home/u");
    assert_eq!(
        crumbs(
            Path::new("/home/u/stacks/docs/a.html"),
            Some(root),
            Some(home)
        ),
        ["stacks", "docs", "a.html"]
    );
    assert_eq!(
        crumbs(Path::new("/home/u/other/a.md"), Some(root), Some(home)),
        ["~", "other", "a.md"]
    );
    // A name cannot reorder the text around it.
    let hostile = crumbs(Path::new("/srv/a\u{202e}b.md"), None, None);
    assert!(hostile.iter().all(|name| !name.contains('\u{202e}')));
}

#[test]
fn a_file_gets_its_page_its_source_with_a_reason_or_the_code_view() {
    let home = Some(Path::new("/home/u"));
    let view = |path: &str, embedded| view_for(Path::new(path), home, embedded);
    for path in ["/home/u/site/plan.md", "/srv/site/index.html"] {
        assert!(
            matches!(view(path, true), Ok(FileView::Page(file)) if file.path() == Path::new(path)),
            "{path}"
        );
        // A build without pages shows the same file as its source.
        assert!(
            matches!(view(path, false), Ok(FileView::Source(_, SourceWhy::Build))),
            "{path}"
        );
    }
    // The page rule serves no hidden folder, nor the home folder itself:
    // the file opens all the same, as its source.
    for path in [
        "/home/u/.local/notes/plan.md",
        "/srv/.cache/site/index.html",
        "/home/u/readme.md",
    ] {
        assert!(
            matches!(
                view(path, true),
                Ok(FileView::Source(file, SourceWhy::Folder)) if file.path == path
            ),
            "{path}"
        );
    }
    for path in ["/srv/site/main.rs", "/home/u/.config/app/settings.toml"] {
        assert!(matches!(view(path, true), Ok(FileView::Code(_))), "{path}");
    }
    // No tab names a relative path, or one that could draw control text.
    for path in ["notes/plan.md", "", "/srv/a\u{1b}[2J.md"] {
        assert!(
            matches!(view(path, true), Err(crate::Error::InvalidCodeFile)),
            "{path}"
        );
    }
}

#[test]
fn browser_open_shows_a_hidden_folders_page_file_as_source_and_refuses_other_files_there() {
    let home = Some(Path::new("/home/u"));
    let open = |path: &str, embedded, source| browser_view(Path::new(path), home, embedded, source);
    assert!(matches!(
        open("/srv/site/plan.md", true, false),
        Ok((Location::Local { .. }, None))
    ));
    assert!(matches!(
        open("/srv/site/plan.md", true, true),
        Ok((Location::Local { .. }, Some(SourceWhy::Asked)))
    ));
    assert!(matches!(
        open("/home/u/.local/plan.md", true, false),
        Ok((Location::Code { .. }, Some(SourceWhy::Folder)))
    ));
    // An image was asked for as a page: the page rule decides alone.
    assert!(matches!(
        open("/srv/site/shot.png", true, false),
        Ok((Location::Local { .. }, None))
    ));
    assert!(matches!(
        open("/srv/.secrets/shot.png", true, false),
        Err(crate::Error::InvalidLocalPage)
    ));
    // A build without pages says that every file shows as its source.
    assert!(matches!(
        open("/srv/site/plan.md", false, false),
        Ok((Location::Code { .. }, Some(SourceWhy::Build)))
    ));
}
