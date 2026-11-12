//! One active thread for each window reads the saved sessions. A request waits in
//! a slot of its kind: a newer list or detail replaces the waiting one.
use super::super::{Error, history};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

/// The time limit of a list.
pub(crate) const LIST_LIMIT: Duration = Duration::from_secs(5);
/// The time limit of a detail.
pub(crate) const DETAIL_LIMIT: Duration = Duration::from_secs(10);

#[derive(Debug, Default)]
struct Slots {
    list: Option<(u64, Instant)>,
    detail: Option<(u64, String, usize, Instant)>,
}

#[derive(Debug)]
pub(super) enum Answer {
    List {
        generation: u64,
        finished: Instant,
        rows: Vec<history::Row>,
    },
    Detail {
        generation: u64,
        finished: Instant,
        result: Result<Box<history::Detail>, history::Error>,
    },
}

#[derive(Debug)]
pub(super) struct Worker {
    slots: Arc<Mutex<Slots>>,
    wake: mpsc::SyncSender<()>,
    pub(super) answers: mpsc::Receiver<Answer>,
    cancelled: Arc<AtomicBool>,
    thread: thread::JoinHandle<()>,
}

impl Worker {
    /// Asks for a list. Whether the request was placed.
    pub(super) fn list(&self, generation: u64, deadline: Instant) -> bool {
        self.place(|slots| slots.list = Some((generation, deadline)))
    }

    /// Asks for the detail of `id`. Whether the request was placed.
    pub(super) fn detail(
        &self,
        generation: u64,
        id: String,
        ratings: usize,
        deadline: Instant,
    ) -> bool {
        self.place(|slots| slots.detail = Some((generation, id, ratings, deadline)))
    }

    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.wake.try_send(());
    }

    pub(super) fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub(super) fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }

    fn place(&self, set: impl FnOnce(&mut Slots)) -> bool {
        if self.cancelled() {
            return false;
        }
        let Ok(mut slots) = self.slots.try_lock() else {
            return false;
        };
        set(&mut slots);
        let _ = self.wake.try_send(());
        true
    }
}

impl Drop for Worker {
    /// The thread ends after the read it is in, without a join here: a read
    /// past its time limit must not block the window.
    fn drop(&mut self) {
        self.cancel();
    }
}

pub(super) fn start() -> Result<Worker, Error> {
    let (wake, woken) = mpsc::sync_channel(1);
    let slots = Arc::new(Mutex::new(Slots::default()));
    let shared = slots.clone();
    // Two answers at most are in flight: one list, one detail.
    let (sender, answers) = mpsc::sync_channel(4);
    let cancelled = Arc::new(AtomicBool::new(false));
    let stop = cancelled.clone();
    let home = sheprd_client::local_path::home_dir();
    let thread = thread::Builder::new()
        .name("herdr-walkr-history".into())
        .spawn(move || run(home, &woken, &shared, &sender, &stop))
        .map_err(Error::Worker)?;
    Ok(Worker {
        slots,
        wake,
        answers,
        cancelled,
        thread,
    })
}

fn run(
    home: Option<PathBuf>,
    woken: &mpsc::Receiver<()>,
    slots: &Mutex<Slots>,
    answers: &mpsc::SyncSender<Answer>,
    cancelled: &AtomicBool,
) {
    loop {
        if woken.recv().is_err() {
            return;
        }
        loop {
            if cancelled.load(Ordering::Acquire) {
                return;
            }
            let (list, detail) = {
                let Ok(mut slots) = slots.lock() else {
                    return;
                };
                // A detail first: a person waits for it.
                match slots.detail.take() {
                    Some(detail) => (None, Some(detail)),
                    None => (slots.list.take(), None),
                }
            };
            let answer = match (list, detail) {
                (_, Some((generation, id, ratings, deadline))) => {
                    let result = match &home {
                        Some(home) => history::detail(home, &id, ratings, deadline).map(Box::new),
                        None => Err(history::Error::NoHome),
                    };
                    Answer::Detail {
                        generation,
                        finished: Instant::now(),
                        result,
                    }
                }
                (Some((generation, deadline)), None) => {
                    let rows = home
                        .as_deref()
                        .map_or_else(Vec::new, |home| history::list(home, deadline));
                    Answer::List {
                        generation,
                        finished: Instant::now(),
                        rows,
                    }
                }
                (None, None) => break,
            };
            // A window that stopped reading drops the answer, never blocks it.
            let _ = answers.try_send(answer);
        }
    }
}
