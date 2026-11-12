//! A channel-only worker that never probes or finishes during a test.
#![allow(clippy::unwrap_used)]
use super::*;

impl Worker {
    pub(in crate::walkr) fn requested(&self) -> bool {
        self.pending.lock().unwrap().is_some()
    }

    pub(in crate::walkr) fn fake() -> (Self, mpsc::Receiver<()>, mpsc::SyncSender<()>) {
        let (wake, commands) = mpsc::sync_channel(1);
        let (_, updates) = mpsc::sync_channel(2);
        let (release, wait) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || {
            let _ = wait.recv();
        });
        (
            Self {
                wake,
                pending: Arc::new(Mutex::new(None)),
                updates,
                cancelled: Arc::new(AtomicBool::new(false)),
                in_flight: Arc::new(AtomicBool::new(false)),
                thread,
            },
            commands,
            release,
        )
    }
}
