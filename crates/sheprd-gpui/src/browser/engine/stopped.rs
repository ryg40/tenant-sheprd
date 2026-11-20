//! A page whose script stopped. Servo runs the documents of each site of a
//! tab on a script thread of its own, inside the app's process. A panic
//! there ends that thread, and the tab keeps its last picture and takes no
//! input. The pinned Servo learns of such a panic only through the `log`
//! logger that `Servo::setup_logging` installs, which the app does not use;
//! it calls `notify_crashed` only when a later message to that thread
//! fails, as a reload's does.
//!
//! The crash hook contains the panics of Servo's script threads
//! ([`SCRIPT_THREADS`]) and marks the thread. Once that thread has ended,
//! and its queue with it, it sets [`ENDED`] and wakes the engine. Nothing
//! is queued, so a busy engine loses nothing; the engine reads the flag
//! each time it runs, and then asks every page a question ([`probe`]).
//!
//! The signal: Servo answers `JavaScriptEvaluationError::InternalError` in
//! two cases (`Constellation::handle_evaluate_javascript`). Either the
//! question cannot be sent to the script thread of the view's current
//! document, whose queue is gone: the exact word that the thread ended.
//! Or the view has no current document, no browsing context or no pipeline
//! for it, as before its first document is shown. A live thread that has
//! no window for the document answers `WebViewNotReady` instead. So a page
//! is asked only once it has a first history entry (`WebView::url`, which
//! Servo sets after it made the browsing context) and no load of it runs,
//! and its internal error counts only if no load or Back or Forward step
//! started before it answered. Any other internal error tells nothing:
//! the page is asked again as a load or a step of it completes, at most
//! [`RETRIES`] times for one end, never at once and never in a loop. A
//! page whose thread ends while it loads is therefore not told, as that
//! load never completes. An answer that is not an error ends the asking,
//! unless another thread ended after its question.
//!
//! A question waits until the page's thread reads it, which a busy page
//! does late; nothing here counts time. A thread that ends first drops
//! the questions that wait for it, which are never answered. So each end
//! asks every page once more, also one whose questions wait: a question
//! to a thread that is gone fails at once, with the exact answer. At most
//! [`WAITING`] questions wait for one page; an end past that asks once an
//! answer comes. A page cannot make the answer: it is Servo's own, about
//! a thread the hook saw end. The page's window learns it from the page's
//! [`Mailbox`], where it waits until the window takes it, so no full
//! queue loses it.
//!
//! Each tab's view is a browsing context group of its own, and the app
//! opens no window a page asks for, so one script thread serves one tab:
//! its top document and the frames of the same site, or only the frames of
//! another site. The end of a thread that ran only a frame leaves that
//! frame blank and its tab as it is.
use super::{Engine, Mailbox, Notifier};
use servo::{JSValue, JavaScriptEvaluationError, LoadStatus, WebView};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// The start of the names Servo gives its script threads, `Script#<n>`
/// (`ScriptThreadFactory::create` in `servo-script`).
// i18n: data
const SCRIPT_THREADS: &str = "Script#";

/// The question each page is asked, once a script thread ended. Any answer
/// but Servo's internal error means that its document still runs.
// i18n: data
const QUESTION: &str = "0";

/// Loads and steps that complete and ask a page again, after one end,
/// while its answers tell nothing.
const RETRIES: u8 = 3;

/// The questions that may wait for one page's answers at once. Servo keeps
/// each one's callback until its answer comes, so a page that never reads
/// them holds this many small callbacks, no more. A busy page is still
/// told of its own thread's end, unless this many other script threads
/// ended while it was busy: then its questions fill the bound, and the
/// end of its own thread drops them all.
const WAITING: u8 = 4;

/// Whether a script thread ended since the engine last asked its pages.
static ENDED: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// On the engine's thread: whether some page waits to be asked.
    static DUE: Cell<bool> = const { Cell::new(false) };
    static ENDING: Ending = const { Ending(Cell::new(false)) };
}

/// Has the crash hook contain the panics of Servo's script threads: such a
/// panic stops the pages of one tab, and not the app.
pub(super) fn install() {
    crate::crash::contain(SCRIPT_THREADS, panicked);
}

/// Runs inside the crash hook on a script thread that panicked: one write
/// of the thread's own value, which neither blocks nor panics. Its first
/// use on a thread registers the value's end with the runtime, which the
/// mark needs.
fn panicked(_: &str) {
    // A thread whose own values are already gone has no end left to tell.
    let _ = ENDING.try_with(|ending| ending.0.set(true));
}

/// Set on a script thread that panicked: its end tells the engine.
struct Ending(Cell<bool>);

impl Drop for Ending {
    /// Runs as the thread ends, after its unwinding dropped what Servo kept
    /// on it, its queue included, so that Servo can no longer reach it.
    fn drop(&mut self) {
        if !self.0.get() {
            return;
        }
        ENDED.store(true, Ordering::Release);
        // A full wake queue holds a wake already; an engine that is gone
        // has no pages to ask.
        if let Some(engine) = Engine::started() {
            let _ = engine.wakes.try_send(());
        }
    }
}

/// A word of the engine about a page, kept with the page until its window
/// takes it: that its script thread is gone, or that no tab may hold it.
#[derive(Default)]
pub(in crate::browser) struct Mark(AtomicBool);

impl Mark {
    pub(in crate::browser) fn set(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Whether the mark was set since this was last asked.
    pub(in crate::browser) fn take(&self) -> bool {
        self.0.swap(false, Ordering::AcqRel)
    }
}

/// What the engine asks one page's view, on its thread.
pub(super) struct Probe {
    mailbox: Arc<Mailbox>,
    notifier: Arc<Notifier>,
    /// Script threads that ended while the page was open.
    ends: Cell<u64>,
    /// A script thread ended, and the page has not answered for sure since.
    open: Cell<bool>,
    /// Questions that wait for their answer: [`WAITING`] at most.
    waiting: Cell<u8>,
    /// The page is asked as the engine next runs and a question may wait.
    due: Cell<bool>,
    /// Loads and steps that may still complete and ask again, for the
    /// open end.
    retries: Cell<u8>,
    /// Loads and steps the page started: an answer from before one tells
    /// nothing of the document after it.
    loads: Cell<u64>,
}

/// When a question was asked: after how many loads and ends of the page.
#[derive(Clone, Copy)]
struct Asked {
    loads: u64,
    ends: u64,
}

impl Probe {
    pub(super) fn new(mailbox: Arc<Mailbox>, notifier: Arc<Notifier>) -> Self {
        Self {
            mailbox,
            notifier,
            ends: Cell::new(0),
            open: Cell::new(false),
            waiting: Cell::new(0),
            due: Cell::new(false),
            retries: Cell::new(0),
            loads: Cell::new(0),
        }
    }

    /// Tells the page's window that the page stopped: its answer said so,
    /// or Servo did (`notify_crashed`).
    pub(super) fn stopped(&self) {
        self.mailbox.stopped.set();
        self.notifier.notify();
    }

    /// A load of the page started, or one ended. An end asks again while
    /// the page has not answered for sure and retries are left.
    pub(super) fn loaded(&self, status: LoadStatus) {
        match status {
            LoadStatus::Started => self.loads.set(self.loads.get().wrapping_add(1)),
            LoadStatus::Complete | LoadStatus::Stopped => self.completed(),
            LoadStatus::HeadParsed => {}
        }
    }

    /// A Back or Forward step ended. A step inside one document, or to one
    /// that still runs, loads nothing: no load completes after it.
    pub(super) fn stepped(&self) {
        self.completed();
    }

    fn completed(&self) {
        let left = self.retries.get();
        if self.open.get() && left > 0 {
            self.retries.set(left - 1);
            self.make_due();
        }
    }

    /// A script thread ended: the page is asked, also while questions wait,
    /// as this may be the end of the thread they wait for.
    fn ended(&self) {
        self.ends.set(self.ends.get().wrapping_add(1));
        self.open.set(true);
        self.retries.set(RETRIES);
        self.make_due();
    }

    fn make_due(&self) {
        self.due.set(true);
        DUE.set(true);
    }

    /// Whether to ask the page now, `settled` when it has a document and no
    /// load of it runs. A page that is not settled is asked as a load of it
    /// completes. One with [`WAITING`] questions stays due, and is asked as
    /// one is answered.
    fn ask(&self, settled: bool) -> Option<Asked> {
        if self.waiting.get() >= WAITING || !self.due.replace(false) {
            return None;
        }
        if !self.open.get() || !settled {
            return None;
        }
        self.waiting.set(self.waiting.get() + 1);
        Some(Asked {
            loads: self.loads.get(),
            ends: self.ends.get(),
        })
    }

    /// The answer to the question `asked`.
    fn answered(&self, asked: Asked, answer: &Result<JSValue, JavaScriptEvaluationError>) {
        self.waiting.set(self.waiting.get().saturating_sub(1));
        let internal = matches!(answer, Err(JavaScriptEvaluationError::InternalError));
        if internal && asked.loads == self.loads.get() {
            self.open.set(false);
            tracing::info!("A page stopped: its script thread ended");
            self.stopped();
            return;
        }
        // Any other answer comes from a thread that ran after the ends
        // before the question; an internal error across a load tells
        // nothing.
        if !internal && asked.ends == self.ends.get() {
            self.open.set(false);
        }
        tracing::debug!(
            known = !internal,
            "A page answered whether its script thread ended"
        );
        // An end or a completed load made the page due while it waited.
        if self.due.get() {
            DUE.set(true);
        }
    }
}

/// Asks each page that is due, and every page once a script thread ended.
/// Runs on the engine's thread each time Servo has run.
pub(super) fn probe<'a>(pages: impl IntoIterator<Item = (&'a WebView, &'a Rc<Probe>)>) {
    let ended = ENDED.swap(false, Ordering::AcqRel);
    if !ended && !DUE.get() {
        return;
    }
    for (view, probe) in pages {
        if ended {
            probe.ended();
        }
        ask(view, probe);
    }
    // A page still due waits for its answer, which makes it due again.
    DUE.set(false);
}

/// Asks `view` the [`QUESTION`] if its probe is due and the view has a
/// document that no load is changing.
fn ask(view: &WebView, probe: &Rc<Probe>) {
    let settled = view.url().is_some()
        && matches!(
            view.load_status(),
            LoadStatus::Complete | LoadStatus::Stopped
        );
    let Some(asked) = probe.ask(settled) else {
        return;
    };
    // Servo keeps the callback until the answer comes, which a thread that
    // ended never sends: it holds no page, and a closed view's answer is
    // dropped. Servo runs it inside its own evaluator, which may not be
    // asked again from there: a further question waits for `probe`.
    let probe = Rc::downgrade(probe);
    view.evaluate_javascript(QUESTION, move |answer| {
        if let Some(probe) = probe.upgrade() {
            probe.answered(asked, &answer);
        }
    });
}

#[cfg(test)]
mod tests;
