//! The folders of a local checkout, for the file explorer: one folder's
//! entries at a time, read when the folder is opened, so blocking work for a
//! background executor. Everything is bounded: the entries kept and looked
//! at per folder, the length of a name, how deep a path goes, and the Git
//! calls that mark ignored entries.
//!
//! Names are untrusted. A path is taken only as plain names under the root,
//! every folder on the way must be a real folder, and a symlink is listed
//! but never opened as a folder, so nothing outside the root is read. `.git`
//! is never listed. Entries Git ignores stay in the listing, marked, as an
//! agent's build output is often what the user is looking for.

mod error;

pub use error::Error;

use std::{
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

/// The most entries one folder lists; the rest are counted in [`Omitted`].
pub const MAX_ENTRIES: usize = 2_000;
/// The most entries of a folder looked at to find those, which are the
/// first by name of the ones looked at.
const MAX_SCANNED: usize = 4 * MAX_ENTRIES;
/// The longest name listed, in bytes, which is what most file systems allow.
pub const MAX_NAME_BYTES: usize = 255;
/// The deepest folder listed, in names below the root.
pub const MAX_DEPTH: usize = 64;
/// The longest relative path taken, in bytes.
pub const MAX_PATH_BYTES: usize = 4096;
/// The argument bytes of one `git check-ignore`, under every platform's
/// command line limit, and the most such calls for one folder. Entries
/// beyond them are listed unmarked.
const IGNORE_CHUNK_BYTES: usize = 16 * 1024;
const MAX_IGNORE_CALLS: usize = 8;
/// How long Git may take for one such call, and to find the checkout.
const GIT_DEADLINE: Duration = Duration::from_secs(10);
/// Never listed: Git's own folder, or the file a linked worktree has there.
const GIT_DIR: &str = ".git";

/// What an entry is, as the folder says without following a link.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Folder,
    File,
    /// A symbolic link, shown and never followed.
    Symlink,
    /// A socket, FIFO, or device, which nothing opens.
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub kind: Kind,
    /// Git ignores it, or the folder holding it.
    pub ignored: bool,
}

/// One folder's entries, folders first, then by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    pub entries: Vec<Entry>,
    /// The entries a limit, or a name that cannot be shown, left out.
    pub omitted: Omitted,
}

/// How many entries of a folder its listing leaves out. `.git` is not
/// counted: it is never listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Omitted {
    /// This many, of all the folder holds; zero when every entry is listed.
    Exactly(usize),
    /// At least this many: the folder holds more entries than were looked
    /// at, and those are not counted.
    AtLeast(usize),
}

impl Default for Omitted {
    fn default() -> Self {
        Self::Exactly(0)
    }
}

impl Omitted {
    /// Whether any entry is left out.
    pub fn any(self) -> bool {
        self != Self::Exactly(0)
    }
}

/// The names of `path`, a `/`-separated path relative to a root, or why it
/// is refused: an absolute path, `..`, `.git`, a name that does not show as
/// it reads, or one beyond the limits. The root itself is the empty path.
pub fn names(path: &str) -> crate::Result<Vec<&str>> {
    if path.is_empty() {
        return Ok(Vec::new());
    }
    let names: Vec<&str> = path.split('/').collect();
    let plain = names.iter().all(|name| shown(name));
    if path.len() > MAX_PATH_BYTES || names.len() > MAX_DEPTH || !plain {
        return Err(Error::Outside.into());
    }
    Ok(names)
}

/// Whether a name is listed, and so may be part of a path: one plain name
/// of the platform, bounded, not Git's folder, and without control or
/// direction-override characters. The one rule for what the tree lists and
/// what it opens, so it never lists a name it then refuses.
fn shown(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && !name.eq_ignore_ascii_case(GIT_DIR)
        && !name.chars().any(sheprd_client::text::unsafe_char)
        && matches!(
            Path::new(name).components().collect::<Vec<_>>().as_slice(),
            [Component::Normal(_)]
        )
        && !separates(name)
}

/// Whether `name` holds a character that separates names or names a drive
/// or a stream on this platform, beyond what its path parser splits at.
/// Elsewhere a backslash and a colon are plain characters of a name.
#[cfg(windows)]
fn separates(name: &str) -> bool {
    name.contains(['\\', ':'])
}

#[cfg(not(windows))]
fn separates(_: &str) -> bool {
    false
}

/// The folder `path` names under `root`, once every folder on the way is a
/// real folder and not a link. Blocking.
fn real_folder(root: &Path, path: &str) -> crate::Result<PathBuf> {
    let mut folder = root.to_owned();
    for name in names(path)? {
        folder.push(name);
        let real = std::fs::symlink_metadata(&folder).is_ok_and(|metadata| metadata.is_dir());
        if !real {
            return Err(Error::NotFolder.into());
        }
    }
    Ok(folder)
}

/// Lists folder `path` of `root` (the empty path for the root itself).
/// Blocking: reads one folder, and asks Git which entries it ignores.
pub fn list(root: &Path, path: &str, cancelled: &AtomicBool) -> crate::Result<Listing> {
    let stop = || cancelled.load(Ordering::Acquire);
    let folder = real_folder(root, path)?;
    let mut listing = Listing::default();
    let mut scanned = 0;
    // Entries looked at and left out: unreadable, or a name not shown.
    let mut skipped = 0;
    let mut uncounted = false;
    for entry in std::fs::read_dir(&folder).map_err(Error::Unreadable)? {
        if stop() {
            return Err(Error::Cancelled.into());
        }
        scanned += 1;
        if scanned > MAX_SCANNED {
            uncounted = true;
            break;
        }
        let Ok(entry) = entry else {
            skipped += 1;
            continue;
        };
        let name = entry.file_name();
        if name.eq_ignore_ascii_case(GIT_DIR) {
            continue;
        }
        let Some(name) = name.to_str().filter(|name| shown(name)) else {
            skipped += 1;
            continue;
        };
        let kind = match entry.file_type() {
            Ok(kind) if kind.is_symlink() => Kind::Symlink,
            Ok(kind) if kind.is_dir() => Kind::Folder,
            Ok(kind) if kind.is_file() => Kind::File,
            _ => Kind::Other,
        };
        listing.entries.push(Entry {
            name: name.to_owned(),
            kind,
            ignored: false,
        });
    }
    listing.entries.sort_by(|a, b| {
        let folder = |entry: &Entry| entry.kind != Kind::Folder;
        folder(a)
            .cmp(&folder(b))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    // Cut after the sort, so the entries shown are the first by name, not
    // the first the folder happened to give.
    let cut = listing.entries.len().saturating_sub(MAX_ENTRIES);
    listing.entries.truncate(MAX_ENTRIES);
    listing.omitted = match uncounted {
        false => Omitted::Exactly(skipped + cut),
        true => Omitted::AtLeast(skipped + cut),
    };
    mark_ignored(root, path, &mut listing.entries, &stop);
    Ok(listing)
}

/// Marks the entries Git ignores. Outside a Git checkout, or when Git
/// fails, nothing is marked: the mark only dims a row.
fn mark_ignored(root: &Path, path: &str, entries: &mut [Entry], stop: &impl Fn() -> bool) {
    let Some(checkout) = root.to_str() else {
        return;
    };
    // A root that is no checkout has nothing ignored, and Git is not asked.
    if std::fs::symlink_metadata(root.join(GIT_DIR)).is_err() {
        return;
    }
    let prefix = if path.is_empty() {
        String::new()
    } else {
        format!("{path}/")
    };
    // An ignored folder ignores all it holds, in one call.
    if !path.is_empty()
        && ignored(checkout, &[path.to_owned()], stop).is_some_and(|i| !i.is_empty())
    {
        entries.iter_mut().for_each(|entry| entry.ignored = true);
        return;
    }
    let mut start = 0;
    for _ in 0..MAX_IGNORE_CALLS {
        let mut bytes = 0;
        let chunk: Vec<String> = entries[start..]
            .iter()
            .map(|entry| format!("{prefix}{}", entry.name))
            .take_while(|name| {
                bytes += name.len() + 1;
                bytes <= IGNORE_CHUNK_BYTES
            })
            .collect();
        if chunk.is_empty() {
            return;
        }
        let Some(found) = ignored(checkout, &chunk, stop) else {
            return;
        };
        for (entry, name) in entries[start..].iter_mut().zip(&chunk) {
            entry.ignored = found.iter().any(|ignored| ignored == name);
        }
        start += chunk.len();
    }
}

/// Those of `paths`, relative to `checkout`, that Git ignores; `None` when
/// Git could not say.
fn ignored(checkout: &str, paths: &[String], stop: &impl Fn() -> bool) -> Option<Vec<String>> {
    // Git takes `-z` only with a list on its input, which this process
    // policy does not give a child. Listed names hold no line break, and one
    // Git quotes (a quote mark or a backslash in it) only stays unmarked.
    let mut args = vec!["-c", "core.quotePath=false", "check-ignore", "--"];
    // `./` keeps a name that starts with a colon from reading as pathspec
    // magic, which this command refuses with all it was asked.
    let dotted: Vec<String> = paths.iter().map(|path| format!("./{path}")).collect();
    args.extend(dotted.iter().map(String::as_str));
    // Exit status 1 says none is ignored, which is an answer; a failure
    // writes to the error stream, which is not a list of `paths`.
    let (_, output) = sheprd_github::pull_request::run_bytes(
        &mut crate::git::command(checkout, &args),
        Instant::now() + GIT_DEADLINE,
        stop,
        2 * IGNORE_CHUNK_BYTES,
    )
    .ok()?;
    let found: Vec<String> = output
        .split(|byte| *byte == b'\n')
        .filter_map(|name| std::str::from_utf8(name).ok())
        .filter_map(|name| name.trim_end_matches('\r').strip_prefix("./"))
        .filter(|name| paths.iter().any(|path| path == name))
        .map(str::to_owned)
        .collect();
    Some(found)
}

/// The folder the explorer shows for a pane working in `dir`: the root of
/// the Git checkout holding it, else `dir` itself. Blocking.///
/// The root always holds `dir`. Git's answer is taken only when it is `dir`
/// or a folder above it that holds `.git`: a repository's own configuration
/// (`core.worktree`) can make Git name any folder of the machine. Without
/// such an answer the root is the nearest folder above `dir` that holds
/// `.git`, else `dir`, with its links resolved, as Git resolves them.
pub fn root(dir: &str, cancelled: &AtomicBool) -> crate::Result<PathBuf> {
    let folder = Path::new(dir);
    if !folder.is_absolute() {
        return Err(Error::NotFolder.into());
    }
    let folder = folder
        .canonicalize()
        .ok()
        .filter(|folder| folder.is_dir())
        .ok_or(Error::NotFolder)?;
    let holds_git = |above: &Path| std::fs::symlink_metadata(above.join(GIT_DIR)).is_ok();
    let named = crate::code_index::checkout_root(dir, cancelled)
        .ok()
        .and_then(|root| root.canonicalize().ok())
        .filter(|root| folder.starts_with(root) && holds_git(root));
    if cancelled.load(Ordering::Acquire) {
        return Err(Error::Cancelled.into());
    }
    let held = || folder.ancestors().find(|above| holds_git(above));
    Ok(named.unwrap_or_else(|| held().unwrap_or(&folder).to_owned()))
}

/// Where absolute `path` is under `root`, as a `/`-separated relative
/// path. Refused when it is not inside the root as written, or when a name
/// on the way is not listed. The caller resolves links first; nothing is
/// read here.
pub fn locate(root: &Path, path: &Path) -> crate::Result<String> {
    let outside = || crate::Error::from(Error::Outside);
    let rest = path.strip_prefix(root).map_err(|_| outside())?;
    let parts: Option<Vec<&str>> = rest
        .components()
        .map(|part| match part {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect();
    let relative = parts.ok_or_else(outside)?.join("/");
    names(&relative)?;
    Ok(relative)
}

/// The regular file `path` names under `root`, to open: every folder on the
/// way is a real folder and the file is no link, so what opens is inside
/// the root. Blocking.
pub fn file(root: &Path, path: &str) -> crate::Result<PathBuf> {
    let (folder, name) = path.rsplit_once('/').unwrap_or(("", path));
    names(name)?;
    let file = real_folder(root, folder)?.join(name);
    if name.is_empty() || !std::fs::symlink_metadata(&file).is_ok_and(|metadata| metadata.is_file())
    {
        return Err(Error::NotFile.into());
    }
    Ok(file)
}

#[cfg(test)]
mod tests;
