//! Threads whose panic the app recovers from on its own, such as a thread
//! that runs the script of one browser tab's page, whose end that tab
//! shows. A panic there is no crash of the app: it writes no crash report
//! and no `Sheprd panicked` record, and the default hook does not print it.
//! The log keeps the thread's name and the place of the panic, and not its
//! message, which may hold what the thread was working on, such as a page's
//! text. Every other thread keeps the whole report.
//!
//! Only a thread's first panic is contained. The runtime raises a second
//! one on the same thread when the first reaches a function that cannot
//! unwind, and then ends the process: that one gets the whole report. A
//! build that aborts on panic contains nothing, as every panic ends the
//! process there.
//!
//! What remains: a panic that cannot unwind from its start, such as a
//! failed check of an unsafe precondition, cannot be told apart here
//! (`PanicHookInfo::can_unwind` is not stable). On a contained thread it
//! ends the app with only this module's WARN record and no crash report.
use std::{
    cell::Cell,
    panic::PanicHookInfo,
    sync::{Mutex, PoisonError, TryLockError},
};

/// The start of a contained thread name, with what is told of a panic on
/// such a thread.
struct Contained {
    prefix: &'static str,
    stopped: fn(&str),
}

static CONTAINED: Mutex<Vec<Contained>> = Mutex::new(Vec::new());

thread_local! {
    /// Whether this thread had a panic contained already.
    static CONTAINED_ONCE: Cell<bool> = const { Cell::new(false) };
}

/// Contains, from now on, the panics of threads whose name starts with
/// `prefix`; an empty prefix contains nothing, and neither does a build
/// that aborts on panic.
///
/// `stopped` runs inside the panic hook, on the panicking thread, with its
/// name, before the thread unwinds. It must be an infallible mark: it may
/// not block, wait for a lock, allocate beyond what the mark itself needs,
/// or panic, since a panic there ends the process inside the hook. Only
/// the hook that [`super::install`] sets reads this.
pub fn contain(prefix: &'static str, stopped: fn(&str)) {
    if prefix.is_empty() {
        return;
    }
    CONTAINED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(Contained { prefix, stopped });
}

/// Whether the panic of `info` is on a contained thread, and the first
/// there. If it is, its record is logged and its registrant told.
pub(super) fn contained(info: &PanicHookInfo<'_>) -> bool {
    // Every panic ends the process in such a build: each gets the report.
    if cfg!(panic = "abort") {
        return false;
    }
    let thread = std::thread::current();
    let Some(name) = thread.name() else {
        return false;
    };
    let Some(stopped) = registered(name) else {
        return false;
    };
    // A thread whose own values are gone is ending: it gets the report.
    let again = CONTAINED_ONCE
        .try_with(|once| once.replace(true))
        .unwrap_or(true);
    if again {
        return false;
    }
    let location = info
        .location()
        .map(|location| location.to_string())
        .unwrap_or_default();
    tracing::warn!(thread = name, location, "A contained thread panicked");
    // The record stays if the panic ends the process after all. The wait
    // holds this thread, with the locks it holds, for at most `FLUSH`, as
    // a report's own wait does; a writer that keeps up answers at once.
    crate::flush(super::FLUSH);
    stopped(name);
    true
}

/// What a panic on thread `name` tells, if the app contains that thread.
/// Never waits: a registry another thread holds, or this one held as it
/// panicked, reads as no registration, and the panic gets the report.
fn registered(name: &str) -> Option<fn(&str)> {
    let registry = match CONTAINED.try_lock() {
        Ok(registry) => registry,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(TryLockError::WouldBlock) => return None,
    };
    registry
        .iter()
        .find(|contained| name.starts_with(contained.prefix))
        .map(|contained| contained.stopped)
}
