//! One thread for each counted host reads its waiting sessions when asked:
//! this computer from the file system, an SSH host through the host's own
//! script runner (its shared SSH connection), with the reader of
//! [`super::remote`]. It only reads.
use super::{Error, remote};
use crate::{
    automations::Machine,
    teleport::Host,
    walkr::history::pending::{self, Tally},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

/// The time limit of a read of this computer.
const LOCAL_LIMIT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(super) struct Reader {
    wake: mpsc::SyncSender<()>,
    pub(super) answers: mpsc::Receiver<Result<Tally, Error>>,
    cancelled: Arc<AtomicBool>,
}

impl Reader {
    /// Asks for a read. A read already asked for stays the one asked.
    pub(super) fn ask(&self) {
        let _ = self.wake.try_send(());
    }
}

impl Drop for Reader {
    /// The thread ends after the read it is in, without a join here.
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.wake.try_send(());
    }
}

pub(super) fn start(machine: &Machine, host: Host) -> Result<Reader, Error> {
    let (wake, woken) = mpsc::sync_channel(1);
    let (sender, answers) = mpsc::sync_channel(1);
    let cancelled = Arc::new(AtomicBool::new(false));
    let stop = cancelled.clone();
    let local = machine.is_local();
    thread::Builder::new()
        .name("herdr-walkr-waiting".into())
        .spawn(move || {
            while woken.recv().is_ok() {
                if stop.load(Ordering::Acquire) {
                    return;
                }
                let read = if local {
                    read_local()
                } else {
                    remote::list(&host, remote::DEADLINE, &stop)
                };
                if stop.load(Ordering::Acquire) {
                    return;
                }
                let _ = sender.try_send(read);
            }
        })
        .map_err(Error::Worker)?;
    Ok(Reader {
        wake,
        answers,
        cancelled,
    })
}

fn read_local() -> Result<Tally, Error> {
    let home = sheprd_client::local_path::home_dir().ok_or(Error::NoHome)?;
    pending::local(&home, Instant::now() + LOCAL_LIMIT)
}
