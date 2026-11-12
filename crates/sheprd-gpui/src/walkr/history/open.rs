//! Opens records read-only and stops reads at their time limit.
//!
//! Unix walks from an opened home directory through no-follow directory
//! handles. Replacing a child folder with a symlink cannot redirect a read.
//! The home path itself is trusted; an opened directory can still be moved,
//! and file contents can change during a read. Windows refuses a reparse
//! point at the record handle, but folder replacement between path checks
//! and the record open remains a race; see `docs/walkr.md`.
use super::Error;
use std::{
    fs::{self, File},
    io::{self, Read},
    path::Path,
    time::Instant,
};

const RECORD: &str = "session.json";

/// Whether `path` is a directory itself, not a symlink to one.
pub(super) fn is_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_dir())
}

#[cfg(unix)]
pub(super) fn record(home: &Path, id: &str) -> Result<(File, u64), Error> {
    use rustix::fs::{Mode, OFlags, open, openat};
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut folder = open(home, flags, Mode::empty()).map_err(open_error)?;
    for part in super::SESSIONS.into_iter().chain([id]) {
        folder = openat(&folder, part, flags, Mode::empty()).map_err(open_error)?;
    }
    open_record(&folder, Path::new(RECORD))
}

/// The open step itself must not block on a FIFO swapped in after a check.
#[cfg(unix)]
pub(in crate::walkr) fn open_record(
    folder: impl std::os::fd::AsFd,
    path: &Path,
) -> Result<(File, u64), Error> {
    use rustix::fs::{Mode, OFlags, openat};
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let file = File::from(openat(folder, path, flags, Mode::empty()).map_err(open_error)?);
    checked(file)
}

#[cfg(unix)]
fn open_error(error: rustix::io::Errno) -> Error {
    match error {
        rustix::io::Errno::NOENT => Error::Missing,
        rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR => Error::NotRegular,
        _ => Error::Open(error.into()),
    }
}

#[cfg(windows)]
pub(super) fn record(home: &Path, id: &str) -> Result<(File, u64), Error> {
    use std::os::windows::fs::OpenOptionsExt as _;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

    let mut folder = home.to_path_buf();
    for part in super::SESSIONS.into_iter().chain([id]) {
        folder.push(part);
        if !is_dir(&folder) {
            return Err(Error::NotRegular);
        }
    }
    let path = folder.join(RECORD);
    let before = fs::symlink_metadata(&path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            Error::Missing
        } else {
            Error::Open(error)
        }
    })?;
    if !before.file_type().is_file() {
        return Err(Error::NotRegular);
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(Error::Open)?;
    checked(file)
}

fn checked(file: File) -> Result<(File, u64), Error> {
    let metadata = file.metadata().map_err(Error::Open)?;
    if !metadata.file_type().is_file() {
        return Err(Error::NotRegular);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(Error::NotRegular);
        }
    }
    if metadata.len() > super::detail::LIMIT {
        return Err(Error::TooLarge);
    }
    Ok((file, metadata.len()))
}

/// A read already in a system call cannot be interrupted. The window
/// rejects its answer if it finishes after the request's deadline.
pub(crate) struct Deadline<R> {
    inner: R,
    deadline: Instant,
}

impl<R> Deadline<R> {
    pub(crate) fn new(inner: R, deadline: Instant) -> Self {
        Self { inner, deadline }
    }
}

impl<R: Read> Read for Deadline<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if Instant::now() >= self.deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        self.inner.read(buf)
    }
}
