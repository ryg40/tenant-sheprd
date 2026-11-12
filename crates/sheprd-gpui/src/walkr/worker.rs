//! One probe at a time. Configuration edits replace the pending command.
#[cfg(test)]
#[path = "tests/worker.rs"]
mod tests;

use super::{Error, INTERVAL, probe, probe::Reach};
use crate::browser::WebUrl;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Debug)]
pub(super) enum Command {
    Refresh {
        generation: u64,
        url: WebUrl,
        path: String,
    },
}

#[derive(Debug)]
pub(super) struct Update {
    pub generation: u64,
    pub reach: Reach,
    pub checked: Instant,
}

#[derive(Debug)]
pub(super) struct Worker {
    wake: mpsc::SyncSender<()>,
    pending: Arc<Mutex<Option<Command>>>,
    pub updates: mpsc::Receiver<Update>,
    cancelled: Arc<AtomicBool>,
    in_flight: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl Worker {
    pub fn send(&self, command: Command) -> bool {
        let Ok(mut pending) = self.pending.try_lock() else {
            return false;
        };
        *pending = Some(command);
        let _ = self.wake.try_send(());
        true
    }

    pub fn busy(&self) -> bool {
        self.pending.try_lock().map_or(true, |pending| {
            pending.is_some() || self.in_flight.load(Ordering::Acquire)
        })
    }

    pub fn stop(&self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.wake.try_send(());
    }

    pub fn stopping(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(super) fn start() -> Result<Worker, Error> {
    let (wake, received) = mpsc::sync_channel(1);
    let pending = Arc::new(Mutex::new(None));
    let latest = pending.clone();
    let (sender, updates) = mpsc::sync_channel(2);
    let cancelled = Arc::new(AtomicBool::new(false));
    let shared = cancelled.clone();
    let in_flight = Arc::new(AtomicBool::new(false));
    let active = in_flight.clone();
    let thread = thread::Builder::new()
        .name("herdr-walkr".into())
        .spawn(move || run(received, &latest, sender, &shared, &active))
        .map_err(Error::Worker)?;
    Ok(Worker {
        wake,
        pending,
        updates,
        cancelled,
        in_flight,
        thread,
    })
}

fn run(
    wake: mpsc::Receiver<()>,
    pending: &Mutex<Option<Command>>,
    updates: mpsc::SyncSender<Update>,
    cancelled: &AtomicBool,
    in_flight: &AtomicBool,
) {
    let agent = probe::agent(Duration::from_secs(5));
    if wake.recv().is_err() {
        return;
    }
    let mut command = None;
    loop {
        if cancelled.load(Ordering::Acquire) {
            return;
        }
        {
            let Ok(mut pending) = pending.lock() else {
                return;
            };
            in_flight.store(true, Ordering::Release);
            if let Some(next) = pending.take() {
                command = Some(next);
            }
            // Consume the wake with its command under the same short lock.
            let _ = wake.try_recv();
        }
        let Some(Command::Refresh {
            generation,
            url,
            path,
        }) = &command
        else {
            return;
        };
        let reach = probe::probe(&agent, url, path);
        // A slow UI must not keep a stopped worker alive with a full update queue.
        let _ = updates.try_send(Update {
            generation: *generation,
            reach,
            checked: Instant::now(),
        });
        in_flight.store(false, Ordering::Release);
        if cancelled.load(Ordering::Acquire) {
            return;
        }
        match wake.recv_timeout(INTERVAL) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}
