//! Which view a local file gets: its page, its source with the reason it
//! has no page, or the code view. One decision for `browser open`, the
//! switch in a file's bar, and whatever else opens a file by its path.
//! Pure: it reads the path, never the disk.

use crate::{
    browser::{LocalFile, Location},
    code_view::CodeFile,
    control::SourceWhy,
};
use std::path::Path;

/// The view of a local file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FileView {
    /// A Markdown or HTML file, rendered as a page.
    Page(LocalFile),
    /// A Markdown or HTML file that never renders here: its source, and
    /// why. The reason is never [`SourceWhy::Asked`].
    Source(CodeFile, SourceWhy),
    /// Any other file: the read-only code view.
    Code(CodeFile),
}

impl From<FileView> for Location {
    fn from(view: FileView) -> Self {
        match view {
            FileView::Page(file) => Self::Local { file },
            FileView::Source(file, _) | FileView::Code(file) => Self::Code { file },
        }
    }
}

fn has_extension(path: &Path, known: &[&str]) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            known
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

/// Whether `path` names a Markdown file by its extension.
pub(crate) fn is_markdown(path: &Path) -> bool {
    has_extension(path, &["md", "markdown"])
}

/// Whether `path` names a file that renders as a page: Markdown or HTML, by
/// its extension.
pub(crate) fn renders(path: &Path) -> bool {
    is_markdown(path) || has_extension(path, &["html", "htm"])
}

/// The view the file at `path` gets. `path` must already be absolute and
/// resolved, as [`LocalFile::new`] asks; `home` is the user's home folder,
/// and `embedded` whether this build shows pages inside the window
/// (`browser::EMBEDDED`).
///
/// A Markdown or HTML file is a page where the page rule serves its folder.
/// In a hidden folder, or one that holds the home folder, it is its source:
/// the rule itself does not bend. Every other file is code. A path no tab
/// may name (relative, not UTF-8, or with control characters) is refused.
pub(crate) fn view_for(
    path: &Path,
    home: Option<&Path>,
    embedded: bool,
) -> crate::Result<FileView> {
    let code = CodeFile::at(path)?;
    if !renders(path) {
        return Ok(FileView::Code(code));
    }
    if !embedded {
        return Ok(FileView::Source(code, SourceWhy::Build));
    }
    Ok(match LocalFile::new(path, home) {
        Ok(file) => FileView::Page(file),
        Err(_) => FileView::Source(code, SourceWhy::Folder),
    })
}

/// What `browser open` of the file at `path` shows, and why when that is
/// its source; `source` is whether the caller asked for the source. A file
/// that is neither Markdown nor HTML, such as an image, was asked for as a
/// page and stays one under the page rule; a build without pages shows it
/// as code.
pub(crate) fn browser_view(
    path: &Path,
    home: Option<&Path>,
    embedded: bool,
    source: bool,
) -> crate::Result<(Location, Option<SourceWhy>)> {
    Ok(match view_for(path, home, embedded)? {
        FileView::Code(_) if embedded => {
            let file = LocalFile::new(path, home)?;
            (Location::Local { file }, None)
        }
        FileView::Code(file) => (Location::Code { file }, Some(SourceWhy::Build)),
        FileView::Source(file, why) => (Location::Code { file }, Some(why)),
        view @ FileView::Page(_) => (view.into(), source.then_some(SourceWhy::Asked)),
    })
}
