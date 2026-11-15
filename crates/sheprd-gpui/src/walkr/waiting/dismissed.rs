//! The waiting sessions the user cleared out of the count, in Sheprd only:
//! nothing is written on a host or into a session file. A cleared session
//! leaves the badge, the waiting list and the click of Walkr's icon, and
//! still shows in the history.
//!
//! The set keeps the host and the session ID only. An SSH host is kept as
//! a hash of its target, so the stored set names no host or address. An ID
//! is forgotten once two complete reads of its host in a row do not list
//! it as waiting: one read can miss a record that Walkr rewrites at that
//! moment. A failed or cut read forgets nothing. The chrome preferences
//! store it.
use super::super::{history::pending::Tally, run::is_session_id};
use crate::automations::Machine;
use std::collections::{BTreeMap, BTreeSet};

/// The most hosts the set keeps.
const HOSTS: usize = 64;
/// The most IDs the set keeps, of all hosts: four full hosts.
const IDS: usize = 4 * super::super::history::ENTRY_LIMIT;
/// The stored name of this computer.
const LOCAL: &str = "local";
/// The prefix of the stored name of an SSH host.
const SSH: &str = "ssh-";

#[derive(Clone, Debug, Default)]
pub(crate) struct Dismissed {
    /// The IDs this window forgot and did not dismiss again, per host:
    /// a save keeps the stored IDs of another window that shares the file,
    /// but not these. In memory only.
    forgotten: BTreeMap<String, BTreeSet<String>>,
    hosts: BTreeMap<String, BTreeSet<String>>,
}

// By hand: what this window forgot is no part of the stored set.
impl PartialEq for Dismissed {
    fn eq(&self, other: &Self) -> bool {
        self.hosts == other.hosts
    }
}

impl Eq for Dismissed {}

/// Adds `ids` to the set of `name` in `hosts`, within the bounds. How many
/// were new.
fn add(
    hosts: &mut BTreeMap<String, BTreeSet<String>>,
    name: &str,
    ids: impl IntoIterator<Item = String>,
) -> usize {
    if !hosts.contains_key(name) && hosts.len() >= HOSTS {
        return 0;
    }
    let mut room = IDS.saturating_sub(hosts.values().map(BTreeSet::len).sum());
    let set = hosts.entry(name.to_owned()).or_default();
    let mut added = 0;
    for id in ids.into_iter().filter(|id| is_session_id(id)) {
        if room == 0 {
            break;
        }
        if set.insert(id) {
            added += 1;
            room -= 1;
        }
    }
    if set.is_empty() {
        hosts.remove(name);
    }
    added
}

/// The stored name of `machine`: `local`, or `ssh-` and the FNV-1a hash of
/// its target. A cloud machine is never counted, so it has none.
fn key(machine: &Machine) -> Option<String> {
    match machine {
        Machine::Local => Some(LOCAL.to_owned()),
        Machine::Ssh(target) => {
            let mut hash = 0xcbf2_9ce4_8422_2325_u64;
            for byte in target.bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
            Some(format!("{SSH}{hash:016x}"))
        }
        #[cfg(feature = "cloud")]
        Machine::Cloud { .. } => None,
    }
}

/// Whether `name` is a stored name that [`key`] gives.
fn valid_key(name: &str) -> bool {
    name == LOCAL
        || name.strip_prefix(SSH).is_some_and(|hash| {
            hash.len() == 16
                && hash
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        })
}

impl Dismissed {
    /// Adds `ids` of `machine`, within the bounds. How many were new.
    pub(crate) fn insert<'a>(
        &mut self,
        machine: &Machine,
        ids: impl IntoIterator<Item = &'a str>,
    ) -> usize {
        let Some(name) = key(machine) else {
            return 0;
        };
        self.insert_named(name, ids.into_iter().map(str::to_owned))
    }

    fn insert_named(&mut self, name: String, ids: impl IntoIterator<Item = String>) -> usize {
        let ids: Vec<String> = ids.into_iter().collect();
        if let Some(forgotten) = self.forgotten.get_mut(&name) {
            for id in &ids {
                forgotten.remove(id);
            }
        }
        self.forgotten.retain(|_, set| !set.is_empty());
        add(&mut self.hosts, &name, ids)
    }

    /// The IDs of `machine` that a complete read, which listed `open`
    /// (sorted) as waiting, did not list.
    pub(crate) fn missing(&self, machine: &Machine, open: &[String]) -> BTreeSet<String> {
        key(machine)
            .and_then(|name| self.hosts.get(&name))
            .into_iter()
            .flatten()
            .filter(|id| open.binary_search(id).is_err())
            .cloned()
            .collect()
    }

    /// A complete read of `machine` listed `open` (sorted) as waiting. Its
    /// IDs that this read and the one before (`missed`) both did not list
    /// are forgotten; `missed` becomes what this read did not list.
    /// Whether any was forgotten.
    pub(crate) fn prune(
        &mut self,
        machine: &Machine,
        open: &[String],
        missed: &mut BTreeSet<String>,
    ) -> bool {
        let mut missing = self.missing(machine, open);
        let gone: Vec<String> = missing.intersection(missed).cloned().collect();
        if let Some(name) = key(machine)
            && let Some(set) = self.hosts.get_mut(&name)
        {
            for id in &gone {
                set.remove(id);
                missing.remove(id);
            }
            add(&mut self.forgotten, &name, gone.iter().cloned());
        }
        self.hosts.retain(|_, set| !set.is_empty());
        *missed = missing;
        !gone.is_empty()
    }

    /// The IDs of `other` join these, within the bounds, except those this
    /// set forgot: `other` is the stored set, which may still hold them.
    pub(crate) fn merge(&mut self, other: Self) {
        for (name, ids) in other.hosts {
            let forgotten = self.forgotten.get(&name);
            let ids: Vec<String> = ids
                .into_iter()
                .filter(|id| forgotten.is_none_or(|forgotten| !forgotten.contains(id)))
                .collect();
            add(&mut self.hosts, &name, ids);
        }
    }

    /// What `read` of `machine` shows once its dismissed sessions are out:
    /// the count, the list, and the newest page.
    pub(crate) fn shown(&self, machine: &Machine, read: &Tally) -> Tally {
        let mut tally = read.clone();
        let Some(set) = key(machine).and_then(|name| self.hosts.get(&name)) else {
            return tally;
        };
        let out = read.open.iter().filter(|id| set.contains(*id)).count();
        tally.waiting = read.waiting.saturating_sub(out);
        tally.newest.retain(|pending| !set.contains(&pending.id));
        tally
    }

    /// The stored set: an object of host names, each with its sorted IDs.
    pub(crate) fn to_json(&self) -> serde_json::Value {
        self.hosts
            .iter()
            .map(|(name, ids)| (name.clone(), serde_json::json!(ids)))
            .collect::<serde_json::Map<_, _>>()
            .into()
    }

    /// The set stored as `value`. A malformed host or ID is forgotten
    /// rather than failing the whole file, and the bounds apply.
    pub(crate) fn from_json(value: Option<&serde_json::Value>) -> Self {
        let mut dismissed = Self::default();
        for (name, ids) in value
            .and_then(serde_json::Value::as_object)
            .into_iter()
            .flatten()
        {
            let Some(ids) = ids.as_array().filter(|_| valid_key(name)) else {
                continue;
            };
            let ids = ids
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned);
            dismissed.insert_named(name.clone(), ids);
        }
        dismissed
    }
}
