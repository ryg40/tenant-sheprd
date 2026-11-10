//! What sites keep in Servo tabs from one run of the app to the next:
//! cookies, the list of sites that ask for HTTPS, and site storage
//! (`localStorage`, IndexedDB). Servo keeps it in one folder it is given,
//! and without one a restart signs every tab out of every site and a
//! restored tab cannot finish a sign-in it began.
//!
//! The folder is private to the user: others cannot enter it. One process
//! has it at a time: the engine that opens it holds a lock on it until it
//! ends, and the engine of a second Sheprd keeps nothing between runs.
//! Servo writes its cookies only as it shuts down, so a run that is killed
//! keeps the cookies of the run before it.
//!
//! Clearing removes the folder's contents. Servo holds its storage files
//! open while it runs, so with a running engine the clearing removes at
//! once what Servo wrote when it last shut down, the cookies first, and
//! leaves a mark; the rest goes when the engine ends, or at the next start
//! if the run is killed. The cookies of before the clearing are off the
//! disk from that moment, whatever ends the run. The cookie file the
//! engine writes as it shuts down stays, if the engine did drop its
//! cookies: it holds only cookies set after the clearing.
use rustix::{
    fd::OwnedFd,
    fs::{FlockOperation, Mode, OFlags, flock},
    io::Errno,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
};

mod error;

pub(crate) use error::Error;

/// The folder's name in the app's state directory.
const FOLDER: &str = "browser-data";

/// Servo's file of cookies, which it writes whole as it shuts down.
const COOKIES: &str = "cookie_jar.json";

/// The files Servo reads when it starts and writes whole as it shuts down,
/// and never opens in between: a clearing removes them under a running
/// engine. The cookies come first.
const WRITTEN_AT_EXIT: [&str; 3] = [COOKIES, "auth_cache.json", "hsts_list.json"];

/// Marks a folder whose contents wait to be removed.
/// It holds [`KEEP_COOKIES`] once the running engine dropped its cookies.
const PENDING: &str = "clear-pending";

/// What the mark holds when the cookie file may stay.
const KEEP_COOKIES: &[u8] = b"keep-cookies";

/// What a clearing did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cleared {
    /// Everything is gone.
    Now,
    /// The cookies of before are off the disk. A running engine still
    /// holds data in memory and in the files it has open; those go when
    /// it ends, or when an engine next starts.
    AtNextStart,
}

/// One process's hold on the folder: the lock its engine took when it
/// opened the folder, kept until the engine ends.
#[derive(Default)]
struct Hold(Option<OwnedFd>);

/// This process's hold. Its lock is held while the folder is opened or
/// emptied, so neither happens during the other.
static HOLD: Mutex<Hold> = Mutex::new(Hold(None));

/// Where Servo keeps site data, if the app has a state directory.
pub(super) fn folder() -> Option<PathBuf> {
    crate::preferences::state_dir().map(|dir| dir.join(FOLDER))
}

/// Takes the lock on `dir` that says an engine has it. `None` when
/// another process holds it. The lock is on the folder itself, so no
/// clearing removes it, and it ends with the process that took it.
fn lock(dir: &Path) -> Result<Option<OwnedFd>, Error> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let held = rustix::fs::open(dir, flags, Mode::empty())?;
    match flock(&held, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Ok(Some(held)),
        Err(Errno::WOULDBLOCK) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Has the disk keep what was removed from `dir`, so a power loss does not
/// bring it back.
fn sync(dir: &Path) -> Result<(), Error> {
    Ok(fs::File::open(dir)?.sync_all()?)
}

/// Makes `dir` a folder only its owner can enter. A folder that is a link,
/// or another user's, is refused: Servo would write cookies where someone
/// else chose.
fn secure(dir: &Path) -> Result<(), Error> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let meta = fs::symlink_metadata(dir)?;
    if !meta.file_type().is_dir() || meta.uid() != rustix::process::geteuid().as_raw() {
        return Err(Error::NotOwn);
    }
    Ok(fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?)
}

/// Empties `dir` if a clearing waits there. No engine may have it open.
fn settle(dir: &Path) -> Result<(), Error> {
    let mark = dir.join(PENDING);
    let Ok(meta) = mark.symlink_metadata() else {
        return Ok(());
    };
    // Anything but the app's own word removes the cookies too.
    let keep = meta.file_type().is_file()
        && meta.len() == KEEP_COOKIES.len() as u64
        && fs::read(&mark).is_ok_and(|held| held == KEEP_COOKIES);
    empty(dir, keep)
}

/// Removes what is in `dir`, and with `keep_cookies` all but the cookie
/// file. A link in the folder is removed, never followed. The mark of a
/// waiting clearing goes last, so a clearing that fails half way runs again.
fn empty(dir: &Path, keep_cookies: bool) -> Result<(), Error> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == PENDING || (keep_cookies && name == COOKIES) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry.path())?;
        } else {
            fs::remove_file(entry.path())?;
        }
    }
    gone(fs::remove_file(dir.join(PENDING)))?;
    sync(dir)
}

/// A removal of a file that may not be there.
fn gone(removed: io::Result<()>) -> Result<(), Error> {
    match removed {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

/// Starts a clearing of a folder an engine has open. The mark comes first,
/// so a run that ends here has the next start remove everything. Then the
/// files Servo wrote when it last shut down go, at once and for good: the
/// engine does not read them again, and a run that is killed can no longer
/// bring the cookies of before back.
fn pend(dir: &Path) -> Result<(), Error> {
    fs::write(dir.join(PENDING), b"")?;
    for name in WRITTEN_AT_EXIT {
        gone(fs::remove_file(dir.join(name)))?;
    }
    sync(dir)
}

impl Hold {
    /// Opens `dir` for the engine that starts now: makes it private, takes
    /// its lock, finishes a clearing that waits, and closes the files of
    /// earlier runs to others. [`Error::InUse`] when another process's
    /// engine has the folder.
    fn open(&mut self, dir: &Path) -> Result<(), Error> {
        use std::os::unix::fs::PermissionsExt;
        secure(dir)?;
        if self.0.is_none() {
            self.0 = Some(lock(dir)?.ok_or(Error::InUse)?);
        }
        settle(dir)?;
        // Servo creates its files readable by others. The folder keeps
        // others out; the files of earlier runs are closed to them as well.
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o600))?;
            }
        }
        Ok(())
    }

    /// Clears `dir`: all of it at once when no engine has it open, in this
    /// process or another, and else what [`pend`] removes.
    fn clear(&self, dir: &Path) -> Result<Cleared, Error> {
        match fs::symlink_metadata(dir) {
            Ok(meta) if meta.file_type().is_dir() => {}
            // Nothing was ever kept.
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Cleared::Now),
            Ok(_) => return Err(Error::NotOwn),
            Err(error) => return Err(error.into()),
        }
        // Without this process's hold, the lock tells whether another
        // process's engine has the folder; it is kept while the folder is
        // emptied, so no engine opens it meanwhile.
        let free = match &self.0 {
            Some(_) => None,
            None => lock(dir)?,
        };
        if free.is_none() {
            pend(dir)?;
            return Ok(Cleared::AtNextStart);
        }
        empty(dir, false)?;
        Ok(Cleared::Now)
    }

    /// The engine that had `dir` open has ended and written its files:
    /// finishes a clearing that waits, and gives the folder up.
    fn close(&mut self, dir: &Path) -> Result<(), Error> {
        if self.0.take().is_some() {
            settle(dir)?;
        }
        Ok(())
    }
}

/// Records in `dir` that the running engine dropped its cookies after a
/// clearing, so the cookie file it writes as it shuts down stays. Without
/// a mark there is nothing to record.
fn keep_cookies_in(dir: &Path) -> Result<(), Error> {
    let mark = dir.join(PENDING);
    match mark.symlink_metadata() {
        Ok(meta) if meta.file_type().is_file() => Ok(fs::write(mark, KEEP_COOKIES)?),
        _ => Ok(()),
    }
}

fn hold() -> std::sync::MutexGuard<'static, Hold> {
    HOLD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Opens the app's folder for the engine that starts now, which keeps it
/// until it ends. `None`, and Servo keeps nothing between runs, when the
/// app has no state directory, the folder cannot be made private, or
/// another Sheprd's engine has it.
pub(super) fn open() -> Option<PathBuf> {
    let dir = folder()?;
    match hold().open(&dir) {
        Ok(()) => Some(dir),
        Err(error) => {
            tracing::warn!(%error, "Servo tabs keep no cookies between runs");
            None
        }
    }
}

/// Removes what sites kept in `dir`. Reads and writes the disk: never call
/// it on the UI thread. After [`Cleared::AtNextStart`] a running engine
/// must drop what it holds ([`super::clear_data`] does both).
pub(super) fn clear(dir: &Path) -> Result<Cleared, Error> {
    hold().clear(dir)
}

/// Tells `dir` that the running engine dropped its cookies.
pub(super) fn cookies_dropped(dir: &Path) -> Result<(), Error> {
    let _hold = hold();
    keep_cookies_in(dir)
}

/// The engine's thread has ended: finishes a clearing that waits, so the
/// disk is clean once the app has quit.
pub(super) fn engine_ended() {
    let Some(dir) = folder() else {
        return;
    };
    if let Err(error) = hold().close(&dir) {
        tracing::warn!(%error, "Could not finish clearing the data of browser tabs");
    }
}

#[cfg(test)]
mod tests;
