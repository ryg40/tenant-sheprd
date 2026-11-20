//! When a process started, as finely as the OS says: what tells the process
//! a row listed from a later process that was given its pid.
//!
//! Linux counts a process's start in clock ticks since boot (field 22 of
//! `/proc/<pid>/stat`, a hundredth of a second), so a pid reused in the
//! same second still reads differently. macOS says no more than whole
//! seconds through `sysinfo`, and the finer `proc_pidinfo` is an unsafe
//! call, so there a pid reused within the second of the listed start
//! cannot be told apart; nothing here pretends otherwise.

use crate::processes::Identity;

/// A process's start: Linux clock ticks since boot, macOS whole seconds
/// since the epoch. Only ever compared with another read on this computer.
#[cfg_attr(not(unix), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Birth(pub(super) u64);

/// A process the user is asked about: the row's identity, and the start a
/// pid must still read for a signal to go to it. `None` when the listing
/// could not read one, and then nothing is signalled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Victim {
    pub identity: Identity,
    pub birth: Option<Birth>,
}

/// Whether [`early`] reads anything here, so a listing can bracket its
/// table of processes between two reads.
#[cfg(unix)]
pub(super) const BRACKETS: bool = cfg!(target_os = "linux");

/// The start time of a `/proc/<pid>/stat` record. The process's name comes
/// second, in parentheses, and may itself hold spaces, parentheses, and
/// bytes that are no text at all (the kernel cuts a name at fifteen bytes,
/// through a character if need be), so the record is read as bytes and the
/// fields are counted from the last `)`: the state is the third, the start
/// time the twenty-second.
#[cfg(target_os = "linux")]
pub(super) fn ticks(stat: &[u8]) -> Option<Birth> {
    let name_end = stat.iter().rposition(|byte| *byte == b')')?;
    let start = stat[name_end + 1..]
        .split(u8::is_ascii_whitespace)
        .filter(|field| !field.is_empty())
        .nth(19)?;
    std::str::from_utf8(start).ok()?.parse().ok().map(Birth)
}

/// The start of the process `pid` names now, read without `sysinfo`.
/// Blocking: a worker's call.
#[cfg(target_os = "linux")]
pub(super) fn early(pid: u32) -> Option<Birth> {
    ticks(&std::fs::read(format!("/proc/{pid}/stat")).ok()?)
}

/// macOS has nothing finer than what `sysinfo` reads.
#[cfg(all(unix, not(target_os = "linux")))]
pub(super) fn early(_pid: u32) -> Option<Birth> {
    None
}

/// The start of the process `pid` names now; `seconds` is what `sysinfo`
/// read of it. Blocking: a worker's call.
#[cfg(target_os = "linux")]
pub(super) fn now(pid: u32, _seconds: u64) -> Option<Birth> {
    early(pid)
}

/// The start of the process `pid` names now: `seconds`, which `sysinfo`
/// read of it and leaves at zero when the OS would not say.
#[cfg(all(unix, not(target_os = "linux")))]
pub(super) fn now(_pid: u32, seconds: u64) -> Option<Birth> {
    (seconds > 0).then_some(Birth(seconds))
}
