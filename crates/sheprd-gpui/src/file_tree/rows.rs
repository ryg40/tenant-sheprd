//! The rows the panel draws, prepared from the folders read so far.

use super::Folder;
use crate::browser::file_view::{is_markdown, renders};
use gpui::SharedString;
use sheprd_vcs::file_tree::{Kind, Omitted};
use std::{
    collections::{BTreeSet, HashMap},
    path::Path,
};

/// The most rows prepared, however many folders are open.
const MAX_ROWS: usize = 20_000;

/// What kind of file a row names: the icon of its row. The names are those
/// the file view renders. The icon follows the kind, not the view the file
/// opens in: a Markdown file that opens as its source keeps the page icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileKind {
    Markdown,
    Html,
    Other,
}

impl FileKind {
    /// The kind of the file named `name`, by its extension.
    pub(crate) fn of(name: &str) -> Self {
        let path = Path::new(name);
        if is_markdown(path) {
            Self::Markdown
        } else if renders(path) {
            Self::Html
        } else {
            Self::Other
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum What {
    Folder {
        open: bool,
    },
    File(FileKind),
    /// A symbolic link, which is shown and never followed.
    Link,
    /// A socket, FIFO, or device.
    Other,
    /// The folder holds more than is listed: this many more.
    More(Omitted),
    /// The tree has more rows than are prepared.
    Cut,
    Loading,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Row {
    /// Relative to the root. A row that is no entry (more, loading,
    /// failed) has a path no entry can have.
    pub(super) path: String,
    pub(super) label: SharedString,
    pub(super) depth: usize,
    pub(super) what: What,
    pub(super) ignored: bool,
}

/// The tree's rows: the root's entries, and under each open folder its own.
pub(super) fn tree(folders: &HashMap<String, Folder>, expanded: &BTreeSet<String>) -> Vec<Row> {
    let mut rows = Vec::new();
    push_folder(&mut rows, folders, expanded, "", 0);
    if rows.len() >= MAX_ROWS {
        // The rows cut off are not shown; the tree says so at its end.
        rows.push(note("\u{0}", 0, What::Cut));
    }
    rows
}

fn note(folder: &str, depth: usize, what: What) -> Row {
    // A control character is in no listed name.
    Row {
        path: format!("{folder}/\u{0}"),
        label: SharedString::default(),
        depth,
        what,
        ignored: false,
    }
}

fn push_folder(
    rows: &mut Vec<Row>,
    folders: &HashMap<String, Folder>,
    expanded: &BTreeSet<String>,
    folder: &str,
    depth: usize,
) {
    let listing = match folders.get(folder) {
        Some(Folder::Loaded(listing)) => listing,
        Some(Folder::Loading) | None => return rows.push(note(folder, depth, What::Loading)),
        Some(Folder::Failed(error)) => {
            return rows.push(note(folder, depth, What::Failed(error.clone())));
        }
    };
    for entry in &listing.entries {
        if rows.len() >= MAX_ROWS {
            return;
        }
        let path = if folder.is_empty() {
            entry.name.clone()
        } else {
            format!("{folder}/{}", entry.name)
        };
        let open = entry.kind == Kind::Folder && expanded.contains(&path);
        rows.push(Row {
            label: entry.name.clone().into(),
            depth,
            what: match entry.kind {
                Kind::Folder => What::Folder { open },
                Kind::File => What::File(FileKind::of(&entry.name)),
                Kind::Symlink => What::Link,
                Kind::Other => What::Other,
            },
            ignored: entry.ignored,
            path: path.clone(),
        });
        if open {
            push_folder(rows, folders, expanded, &path, depth + 1);
        }
    }
    if listing.omitted.any() {
        rows.push(note(folder, depth, What::More(listing.omitted)));
    }
}

/// A search's rows: each file found, by its path.
pub(super) fn found(hits: &[String]) -> Vec<Row> {
    hits.iter()
        .map(|path| Row {
            path: path.clone(),
            label: path.clone().into(),
            depth: 0,
            what: What::File(FileKind::of(path)),
            ignored: false,
        })
        .collect()
}
