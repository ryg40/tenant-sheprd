//! Servo, the engine that draws pages on Linux (the `servo` feature).
//!
//! One `Servo` serves the whole app. It lives on a thread of its own with
//! every page's `WebView`, none of which may leave the thread that made
//! them: so painting a frame, reading it back, and Servo's shutdown, which
//! waits for its own threads, never run on the UI thread. Windows reach it
//! through two bounded queues of [`Command`]s: one for what the pointer and
//! the keys do in a page, and one, read first, for everything else, so no
//! burst of input pushes out a page's opening or closing. A page's frames
//! come back through its [`Mailbox`], which holds only the latest one, and
//! its reports through the window's bounded queue of
//! [`super::reports::Event`]s.
//!
//! Servo runs inside the app's process: a page that crashes the engine ends
//! the app, where wry's views are processes of their own.
//!
//! Servo asks to run through its waker; the thread sleeps until then or
//! until a command arrives, and paints a page only when Servo reports a new
//! frame for it. Nothing polls.
use super::reports::{Event, Source};
use crossbeam_channel::{Sender, TrySendError};
use std::{
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex, OnceLock, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    task::{Poll, Waker},
    time::Duration,
};

mod console;
pub(super) mod controls;
mod error;
mod host;
pub(super) mod keys;
pub(crate) mod profile;

pub(crate) use error::Error;
pub(crate) use profile::Cleared;

/// Presses, keys, and wheel turns waiting for the engine. A full queue
/// drops what arrives rather than block the window. Pointer moves take one
/// place per page, however many wait ([`Latest`]).
const INPUTS: usize = 1024;

/// Every other command waiting for the engine: a page opened, closed, shown,
/// sized, or loaded. They are few, a person's or an agent's own acts, so the
/// queue does not fill while the engine lives.
const CONTROLS: usize = 256;

/// How long the process waits for Servo to shut down once its event loop
/// has ended and its windows are gone ([`finish`]). A release build needs
/// about 130 ms with one page and 150 ms with five, which GPUI's own wait
/// for quit handlers (200 ms) covers; this is for a shutdown slower than
/// that.
const QUIT_WAIT: Duration = Duration::from_secs(2);

/// How long a clearing waits for the running engine to drop its cookies.
/// Servo answers in a few milliseconds; a busy engine keeps its cookies
/// and the next start removes them.
const FORGET_WAIT: Duration = Duration::from_secs(5);

/// Names one page to the engine, across every window.
pub(super) type PageKey = u64;

/// One drawn frame of a page: BGRA, as GPUI's images are, in device pixels.
pub(super) struct Frame {
    pub(super) width: u32,
    pub(super) height: u32,
    /// Device pixels per point when it was drawn.
    pub(super) scale: f32,
    pub(super) bgra: Vec<u8>,
}

/// The latest frame of one page. A newer frame replaces one the window has
/// not taken yet, so a slow window never queues frames. With it, the list
/// the page has open, if it has one.
#[derive(Default)]
pub(super) struct Mailbox {
    frame: Mutex<Option<Frame>>,
    pub(super) list: controls::Listed,
}

impl Mailbox {
    fn put(&self, frame: Frame) {
        *self.frame.lock().unwrap_or_else(PoisonError::into_inner) = Some(frame);
    }

    pub(super) fn take(&self) -> Option<Frame> {
        self.frame
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

/// Wakes one window when any of its pages has a new frame. Many frames
/// before the window runs wake it once.
#[derive(Default)]
pub(super) struct Notifier {
    pending: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

impl Notifier {
    fn notify(&self) {
        self.pending.store(true, Ordering::Release);
        let waker = self
            .waker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Resolves once a page has a frame the window has not been woken for.
    pub(super) async fn notified(&self) {
        std::future::poll_fn(|cx| {
            if self.pending.swap(false, Ordering::AcqRel) {
                return Poll::Ready(());
            }
            *self.waker.lock().unwrap_or_else(PoisonError::into_inner) = Some(cx.waker().clone());
            // A frame may have landed between the check and the waker.
            if self.pending.swap(false, Ordering::AcqRel) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }
}

/// What only the newest of counts, for one page: where the pointer is and
/// what size the page has. The window overwrites them and queues one
/// [`Command::Moved`] or [`Command::Sized`] when it fills an empty place, so
/// a slow engine sees the latest of each and its queues never fill with them.
#[derive(Default)]
pub(super) struct Latest {
    /// The pointer, in the page's device pixels.
    moved: Mutex<Option<(f32, f32)>>,
    /// The page's size in device pixels, and device pixels per point.
    sized: Mutex<Option<(u32, u32, f32)>>,
}

impl Latest {
    /// Keeps `value` in `place`. Whether the place was empty, so that
    /// nothing queued will read it yet.
    fn put<T>(place: &Mutex<Option<T>>, value: T) -> bool {
        place
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(value)
            .is_none()
    }

    fn take<T>(place: &Mutex<Option<T>>) -> Option<T> {
        place.lock().unwrap_or_else(PoisonError::into_inner).take()
    }

    pub(super) fn put_moved(&self, point: (f32, f32)) -> bool {
        Self::put(&self.moved, point)
    }

    pub(super) fn take_moved(&self) -> Option<(f32, f32)> {
        Self::take(&self.moved)
    }

    pub(super) fn put_sized(&self, size: (u32, u32, f32)) -> bool {
        Self::put(&self.sized, size)
    }

    pub(super) fn take_sized(&self) -> Option<(u32, u32, f32)> {
        Self::take(&self.sized)
    }
}

/// A page to make.
pub(super) struct Open {
    pub(super) key: PageKey,
    pub(super) url: String,
    /// The folder a local page may read, which it keeps wherever it goes.
    pub(super) root: Option<PathBuf>,
    /// Whether the page gets the bridge: only a local page does.
    pub(super) bridge: bool,
    pub(super) light: Option<bool>,
    /// Whether page-controlled console text may reach the app log.
    pub(super) log_console: bool,
    pub(super) source: Source,
    pub(super) reports: SyncSender<Event<Source>>,
    pub(super) mailbox: Arc<Mailbox>,
    pub(super) notifier: Arc<Notifier>,
    pub(super) latest: Arc<Latest>,
}

/// What a script answered, as JSON; runs on the engine's thread.
pub(super) type Answered = Box<dyn FnOnce(String) + Send>;

pub(super) enum Command {
    Open(Box<Open>),
    Close(PageKey),
    Load(PageKey, String),
    Reload(PageKey),
    /// Back, or forward when true.
    History(PageKey, bool),
    /// The page has a new size in its [`Latest`].
    Sized(PageKey),
    /// The pointer moved over the page, to the place in its [`Latest`].
    Moved(PageKey),
    Shown(PageKey, bool),
    Focused(PageKey, bool),
    Zoom(PageKey, f32),
    Light(PageKey, bool),
    Input(PageKey, Box<servo::InputEvent>),
    /// One of the app's own scripts, with nothing to answer.
    Script(PageKey, String),
    Evaluate(PageKey, String, Answered),
    /// The window's answer to the page's list of that number
    /// ([`controls::Menu`]): the options chosen, or none when it was closed.
    Chosen(PageKey, u64, Option<Vec<usize>>),
    /// Drop every site's cookies, site storage, and cached answers, and
    /// say so once done.
    Forget(SyncSender<()>),
    /// The kit's `theme.css` for local pages.
    Theme(String),
    Quit,
}

impl Command {
    /// Whether this is what the pointer or the keys did in a page, which
    /// waits in the queue a burst may fill.
    fn is_input(&self) -> bool {
        matches!(self, Self::Input(..) | Self::Moved(_))
    }
}

/// The app's handle to the engine's thread.
#[derive(Clone)]
pub(super) struct Engine {
    controls: Sender<Command>,
    inputs: Sender<Command>,
    /// Holds at most one wake: Servo asking twice before the thread runs
    /// is one run.
    wakes: Sender<()>,
    /// Whether the thread's end needs no report: the app quits, or a window
    /// was already told.
    ended: Arc<AtomicBool>,
    /// Set by the thread once Servo has shut down.
    stopped: Arc<Stopped>,
}

/// Whether the engine's thread has finished, for the app's quit to wait on.
#[derive(Default)]
pub(super) struct Stopped {
    state: Mutex<(bool, Option<Waker>)>,
    changed: Condvar,
}

impl Stopped {
    fn set(&self) {
        let waker = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.0 = true;
            state.1.take()
        };
        self.changed.notify_all();
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Resolves once the thread has finished. Nothing waits in a thread.
    async fn wait(&self) {
        std::future::poll_fn(|cx| {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.0 {
                return Poll::Ready(());
            }
            state.1 = Some(cx.waker().clone());
            Poll::Pending
        })
        .await;
    }

    /// Waits in the calling thread, at most `limit`. Whether the engine's
    /// thread has finished.
    fn wait_for(&self, limit: Duration) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let (state, _) = self
            .changed
            .wait_timeout_while(state, limit, |state| !state.0)
            .unwrap_or_else(PoisonError::into_inner);
        state.0
    }
}

/// Marks the engine's thread finished when it ends, by a return or a panic.
struct Finished(Arc<Stopped>);

impl Drop for Finished {
    fn drop(&mut self) {
        self.0.set();
    }
}

static ENGINE: OnceLock<Result<Engine, Error>> = OnceLock::new();

impl Engine {
    /// The app's engine, started with the first page. Tests get a handle
    /// that no thread reads: a unit test never starts Servo.
    pub(super) fn get() -> Result<Self, Error> {
        ENGINE.get_or_init(Self::start).clone()
    }

    /// The engine, if a page ever started it.
    pub(super) fn started() -> Option<Self> {
        ENGINE.get()?.as_ref().ok().cloned()
    }

    fn start() -> Result<Self, Error> {
        let (controls, control) = crossbeam_channel::bounded(CONTROLS);
        let (inputs, input) = crossbeam_channel::bounded(INPUTS);
        let (wakes, woken) = crossbeam_channel::bounded(1);
        let waker = host::Wake(wakes.clone());
        let stopped = Arc::new(Stopped::default());
        let engine = Self {
            controls,
            inputs,
            wakes,
            ended: Arc::default(),
            stopped: stopped.clone(),
        };
        // A unit test never starts Servo: its windows get a handle with no
        // thread behind it, and `send` drops what they queue.
        if cfg!(test) {
            return Ok(engine);
        }
        let finished = Finished(stopped);
        std::thread::Builder::new()
            .name("browser-servo".into())
            .spawn(move || {
                let _finished = finished;
                host::run(&control, &input, &woken, waker);
            })
            .map_err(|error| Error::Thread(Arc::new(error)))?;
        Ok(engine)
    }

    /// Queues `command`, without waiting. [`Error::Busy`] when its queue is
    /// full and the command is dropped; [`Error::Stopped`] when the engine's
    /// thread is gone.
    pub(super) fn send(&self, command: Command) -> Result<(), Error> {
        // A test's engine has no thread to read a queue.
        if cfg!(test) {
            return Ok(());
        }
        let queue = if command.is_input() {
            &self.inputs
        } else {
            &self.controls
        };
        match queue.try_send(command) {
            Ok(()) => {
                let _ = self.wakes.try_send(());
                Ok(())
            }
            Err(TrySendError::Full(_)) => Err(Error::Busy),
            Err(TrySendError::Disconnected(_)) => Err(Error::Stopped),
        }
    }

    /// Whether the end of the engine's thread is news: true once, and never
    /// after the app began to quit.
    pub(super) fn report_end(&self) -> bool {
        !self.ended.swap(true, Ordering::AcqRel)
    }
}

/// Makes rustls's `ring` provider the process default. Servo brings
/// `aws-lc-rs` into the same rustls, and with two providers built in rustls
/// picks neither by itself: the app's own TLS clients would panic.
pub(crate) fn choose_tls_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Ends the engine as the app quits. Servo's shutdown waits for its own
/// threads, on the engine's thread. The quit handler only queues the
/// command; its future resolves when the engine's thread has finished, and
/// GPUI gives every quit handler a short time together, so the UI thread
/// never joins the engine. See [`finish`] for a shutdown slower than that.
/// Also names the folder a clearing empties ([`DataFolder`]).
pub(crate) fn install(cx: &mut gpui::App) {
    cx.set_global(DataFolder(profile::folder()));
    cx.on_app_quit(|_| {
        let engine = Engine::started().filter(Engine::quit);
        async move {
            if let Some(engine) = engine {
                engine.stopped.wait().await;
            }
        }
    })
    .detach();
}

impl Engine {
    /// Tells the engine's thread to end, which needs no report from then
    /// on. Whether a thread took the command.
    fn quit(&self) -> bool {
        self.ended.store(true, Ordering::Release);
        self.send(Command::Quit).is_ok()
    }
}

/// The folder Servo keeps what sites store in, for [`clear_data`]: the
/// app's own, named when the app starts. A unit test starts no app, so it
/// has none, and a clearing there touches no folder of the person who runs
/// it.
pub(crate) struct DataFolder(Option<PathBuf>);

impl gpui::Global for DataFolder {}

impl DataFolder {
    /// The folder, if the app has one.
    pub(crate) fn of(cx: &gpui::App) -> Option<PathBuf> {
        cx.try_global::<Self>()?.0.clone()
    }
}

/// Whether an engine runs in this process, so pages may hold site data in
/// memory and in files the engine has open.
pub(crate) fn holds_data() -> bool {
    Engine::started().is_some()
}

/// Removes what sites kept in Servo tabs: cookies and site storage, in
/// `dir` ([`profile`]) and in a running engine. With no engine the folder
/// is emptied and that is all. A running engine drops its cookies, its
/// cached answers, and the web storage it lists at once, and the cookies of
/// before are off the disk at once too; what its pages still hold goes
/// when the engine ends, which [`Cleared::AtNextStart`] says. Waits for the
/// disk and the engine: call it on a background thread, never on the UI
/// thread.
pub(crate) fn clear_data(dir: Option<&std::path::Path>) -> Result<Cleared, Error> {
    let cleared = dir.map_or(Ok(Cleared::Now), profile::clear)?;
    let Some(engine) = Engine::started() else {
        return Ok(cleared);
    };
    let (done, forgotten) = std::sync::mpsc::sync_channel(1);
    match engine.send(Command::Forget(done)) {
        Ok(()) => {}
        // Its thread is gone, and what it held in memory with it.
        Err(Error::Stopped) => return Ok(cleared),
        Err(_) => return Err(profile::Error::Unanswered.into()),
    }
    // An engine that does not answer keeps its cookies for now; the
    // folder's mark then has the cookie file it writes removed as well.
    forgotten
        .recv_timeout(FORGET_WAIT)
        .map_err(|_| profile::Error::Unanswered)?;
    if let Some(dir) = dir {
        profile::cookies_dropped(dir)?;
    }
    Ok(Cleared::AtNextStart)
}

/// Gives Servo the rest of [`QUIT_WAIT`] to shut down before the process
/// ends. Call it after the app's event loop has returned: the windows are
/// closed by then, so nothing a person sees waits. A process that ends
/// while Servo's threads still run crashes in SpiderMonkey's destructors
/// instead of leaving, which costs more time than this wait and leaves a
/// crash report behind.
pub(crate) fn finish() {
    let Some(engine) = Engine::started() else {
        return;
    };
    let asked = engine.quit();
    let wait = if asked { QUIT_WAIT } else { Duration::ZERO };
    if engine.stopped.wait_for(wait) {
        // Servo has written and closed its files.
        profile::engine_ended();
    } else if asked {
        tracing::warn!("Servo did not shut down in time; the process ends without it");
    }
}
