//! The engine's thread: Servo, each page's `WebView`, and what Servo asks
//! of its host. Every handler here applies the page rules of
//! [`crate::browser::policy`]; none of them touches the window.
use super::{Answered, Command, Error, Frame, Open, PageKey, console, controls::Held, profile};
use crate::browser::{
    location::PREVIEW_SCHEME,
    policy::{Verdict, loadable, navigable, posted, verdict},
    preview::{Answer, Preview},
    reports::{Event, report},
};
use crossbeam_channel::{Receiver, Sender, select};
use servo::{
    CreateNewWebViewRequest, DeviceIntPoint, DeviceIntRect, DeviceIntSize, DevicePoint,
    EmbedderControl, EmbedderControlId, EventLoopWaker, InputEvent, JSValue, LoadStatus,
    MouseMoveEvent, NavigationRequest, Opts, Preferences, RenderingContext, Servo, ServoBuilder,
    ServoDelegate, ServoError, SiteData, SoftwareRenderingContext, StorageType, Theme,
    UserContentManager, UserScript, WebResourceLoad, WebResourceResponse, WebView, WebViewBuilder,
    WebViewDelegate, WebViewPoint,
    protocol_handler::{
        DoneChannel, FetchContext, NetworkError, ProtocolHandler, ProtocolRegistry, Request,
        Response,
    },
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{Arc, Mutex, PoisonError},
    thread::ThreadId,
    time::Instant,
};

/// Commands applied before Servo runs again, so a burst of pointer moves
/// does not starve its painting.
const BATCH: usize = 64;

/// Files the preview's worker has read that wait for this thread. The
/// worker waits for a place when all are taken: no answer is dropped.
const ANSWERS: usize = 64;

/// A page's size until its window lays it out.
const FIRST_SIZE: dpi::PhysicalSize<u32> = dpi::PhysicalSize::new(800, 600);

/// Gives every page `window.ipc.postMessage`, as wry does, which the
/// annotation picker and the bridge post through. Servo has no channel from
/// a page to its host, so a message is a request to the preview scheme, which
/// the page's own handler reads and never serves.
// i18n: data
const IPC: &str = "(function () {\
  if (window.ipc) { return; }\
  var post = function (body) {\
    try {\
      fetch('herdr-preview://localhost/.herdr/post?m=' + encodeURIComponent(String(body)),\
        { mode: 'no-cors', cache: 'no-store', credentials: 'omit' }).catch(function () {});\
    } catch (_) {}\
  };\
  Object.defineProperty(window, 'ipc', { value: Object.freeze({ postMessage: post }) });\
})();";

/// Servo's preferences for pages. Servo keeps web features it has not
/// finished behind preferences that are off; each one turned on here is
/// named with the pages that need it, and none is turned on as a group.
fn preferences() -> Preferences {
    Preferences {
        // Pages that load a font from script (`new FontFace`) stop at
        // "FontFace is not defined" without it.
        dom_fontface_enabled: true,
        // Lit-based pages make an `IntersectionObserver` as their modules
        // load, and stay empty without one (Authentik's sign-in page).
        // The preference only exposes the interface to pages.
        dom_intersection_observer_enabled: true,
        // Without `backdrop-filter` a frosted panel or a blurred overlay
        // shows what is behind it unchanged.
        layout_css_backdrop_filter_enabled: true,
        // Pages that ship one variable font draw every weight alike
        // without `font-variation-settings` and the font's weight axis.
        layout_css_font_variations_enabled: true,
        ..Preferences::default()
    }
}

/// Servo's request to run, from any of its threads.
#[derive(Clone)]
pub(super) struct Wake(pub(super) Sender<()>);

impl EventLoopWaker for Wake {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }

    fn wake(&self) {
        // Full means a wake already waits.
        let _ = self.0.try_send(());
    }
}

/// The preview scheme as Servo's network threads see it. Each page answers
/// its own requests first (`Delegate::load_web_resource`), where its folder
/// is known; a request that reaches this handler belongs to no page, and
/// gets nothing. Registered so the scheme counts as secure: an https page's
/// annotation picker may then post to it.
struct NoPage;

impl ProtocolHandler for NoPage {
    fn load<'a>(
        &'a self,
        _: &'a mut Request,
        _: &mut DoneChannel,
        _: &FetchContext,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        Box::pin(std::future::ready(Response::network_error(
            NetworkError::LoadCancelled,
        )))
    }

    fn is_secure(&self) -> bool {
        true
    }
}

/// What the engine itself is asked, for requests that name no page.
struct AppDelegate;

impl ServoDelegate for AppDelegate {
    fn notify_error(&self, error: ServoError) {
        tracing::warn!(?error, "Servo reported an error");
    }

    fn load_web_resource(&self, load: WebResourceLoad) {
        let url = load.request.url.as_url();
        if !loadable(url) || url.scheme() == PREVIEW_SCHEME {
            refuse(load);
        }
    }
}

/// Fails `load` as a network error.
fn refuse(load: WebResourceLoad) {
    let url = load.request.url.as_url().clone();
    load.intercept(WebResourceResponse::new(url)).cancel();
}

/// Answers `load` with the preview scheme's `answer`.
fn respond(load: WebResourceLoad, answer: Answer) {
    let mut headers = ureq::http::HeaderMap::new();
    for (name, value) in answer.headers() {
        // A header that is not valid is left out rather than end the thread.
        if let (Ok(name), Ok(value)) = (
            ureq::http::HeaderName::from_bytes(name.as_bytes()),
            ureq::http::HeaderValue::from_str(value),
        ) {
            headers.insert(name, value);
        }
    }
    let status = ureq::http::StatusCode::from_u16(answer.status)
        .unwrap_or(ureq::http::StatusCode::INTERNAL_SERVER_ERROR);
    let response = WebResourceResponse::new(load.request.url.as_url().clone())
        .headers(headers)
        .status_code(status);
    let mut load = load.intercept(response);
    if !answer.body.is_empty() {
        load.send_body_data(answer.body);
    }
    load.finish();
}

/// Requests of the preview scheme that wait for their answer. Servo's
/// handle to a request (`L`) may not leave this thread, so whoever answers
/// names the request by a number. An answer given on this thread, as the
/// handler runs, waits in `here` until the handler returns; one the preview's
/// worker gives comes through a bounded channel the worker waits on. Either
/// way every request gets its answer, and what waits is bounded by the
/// requests Servo has open.
struct Asked<L = WebResourceLoad> {
    loads: RefCell<HashMap<u64, L>>,
    next: Cell<u64>,
    here: Arc<Mutex<Vec<(u64, Answer)>>>,
    answers: Sender<(u64, Answer)>,
    thread: ThreadId,
    wake: Wake,
}

impl<L> Asked<L> {
    /// For the thread that calls this, which is the one that may answer
    /// without the channel.
    fn new(answers: Sender<(u64, Answer)>, wake: Wake) -> Self {
        Self {
            loads: RefCell::default(),
            next: Cell::new(0),
            here: Arc::default(),
            answers,
            thread: std::thread::current().id(),
            wake,
        }
    }

    /// Keeps `load` until its answer arrives, and returns what hands the
    /// answer over from any thread.
    fn wait(&self, load: L) -> impl FnOnce(Answer) + Send + 'static {
        let number = self.next.replace(self.next.get().wrapping_add(1));
        self.loads.borrow_mut().insert(number, load);
        let (here, answers) = (self.here.clone(), self.answers.clone());
        let (thread, wake) = (self.thread, self.wake.clone());
        move |answer| {
            if std::thread::current().id() == thread {
                // This thread is the channel's only reader: it must not
                // wait on it.
                here.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push((number, answer));
            } else if answers.send((number, answer)).is_ok() {
                wake.wake();
            }
        }
    }

    /// The request `number` names, to answer now.
    fn take(&self, number: u64) -> Option<L> {
        self.loads.borrow_mut().remove(&number)
    }

    /// Hands `respond` each request that was answered on this thread.
    fn settle(&self, respond: impl Fn(L, Answer)) {
        let answered =
            std::mem::take(&mut *self.here.lock().unwrap_or_else(PoisonError::into_inner));
        for (number, answer) in answered {
            if let Some(load) = self.take(number) {
                respond(load, answer);
            }
        }
    }
}

/// What a page's handlers share with the thread's loop.
struct State {
    open: Open,
    preview: Rc<Preview>,
    asked: Rc<Asked>,
    /// Whether Servo has a frame the window has not been given.
    frame_ready: Cell<bool>,
    shown: Cell<bool>,
    scale: Cell<f32>,
    /// The address the tab shows.
    top: RefCell<Option<String>>,
    /// The records the tab's page may still put in the log.
    budget: RefCell<console::Budget>,
    /// The list the page has open, if it has one.
    held: Held,
}

impl State {
    fn report(&self, event: Event<crate::browser::reports::Source>) {
        report(&self.open.reports, event);
    }

    /// Runs `write` when the tab's budget has a place for one more record
    /// of its page, with the count of records that had none before it.
    /// Every record a page can cause without end goes through here.
    fn log(&self, error: bool, write: impl FnOnce(u32)) {
        let place = self.budget.borrow_mut().admit(error, Instant::now());
        let Some(place) = place else {
            return;
        };
        write(place.skipped);
        if place.spent {
            tracing::info!(
                burst = console::BURST,
                "A page used up its tab's log records; later ones are counted until time gives some back"
            );
        }
    }
}

struct Delegate(Rc<State>);

impl WebViewDelegate for Delegate {
    fn notify_new_frame_ready(&self, _: WebView) {
        self.0.frame_ready.set(true);
    }

    fn notify_url_changed(&self, _: WebView, url: url::Url) {
        *self.0.top.borrow_mut() = Some(url.into());
    }

    fn notify_page_title_changed(&self, _: WebView, title: Option<String>) {
        if let Some(title) = title {
            self.0.report(Event::Title(self.0.open.source, title));
        }
    }

    fn notify_load_status_changed(&self, view: WebView, status: LoadStatus) {
        // A list belongs to the page that opened it.
        if matches!(status, LoadStatus::Started) {
            self.0.held.dismiss();
        }
        if matches!(status, LoadStatus::Complete)
            && let Some(url) = view.url()
        {
            self.0.report(Event::Loaded(self.0.open.source, url.into()));
            // After the address, which names the tab then. An answer
            // without content, such as an error status with no body, makes
            // a page without a title: the tab must not keep the last one's.
            if view
                .page_title()
                .is_none_or(|title| title.trim().is_empty())
            {
                self.0.report(Event::Untitled(self.0.open.source));
            }
        }
    }

    fn notify_crashed(&self, _: WebView, reason: String, _: Option<String>) {
        tracing::warn!(reason, "A Servo page crashed");
    }

    /// A page leaves only for web addresses and the preview scheme.
    fn request_navigation(&self, _: WebView, request: NavigationRequest) {
        if navigable(request.url.as_str()) {
            request.allow();
        } else {
            // The address is the page's text and may hold a whole
            // `data:` document or a sign-in code: only its scheme is logged.
            let scheme = request.url.scheme();
            self.0.log(false, |skipped| {
                tracing::debug!(scheme, skipped, "Refused a page's navigation");
            });
            request.deny();
        }
    }

    /// A page opens no window: dropping the request refuses it, and
    /// `window.open` answers `null`.
    fn request_create_new(&self, _: WebView, request: CreateNewWebViewRequest) {
        self.0.log(false, |skipped| {
            tracing::debug!(skipped, "Refused a page's new window");
        });
        drop(request);
    }

    /// Every request of the page passes here before Servo's network sees
    /// it. Files of this computer load only through the preview scheme, and
    /// that scheme serves only the page's own folder to the local page.
    fn load_web_resource(&self, _: WebView, load: WebResourceLoad) {
        let url = load.request.url.as_url();
        let referrer = load.request.referrer_url.as_ref().map(|url| url.as_str());
        let verdict = verdict(
            url,
            referrer,
            load.request.is_for_main_frame,
            self.0.top.borrow().as_deref(),
        );
        match verdict {
            Verdict::Refuse => {
                self.0.log(false, |skipped| {
                    tracing::debug!(scheme = url.scheme(), skipped, "Refused a page's request");
                });
                refuse(load);
            }
            // Servo's network answers.
            Verdict::Network => {}
            Verdict::Post => {
                if let Some((body, from)) = posted(url, referrer) {
                    self.0.report(Event::Posted(self.0.open.source, body, from));
                }
                respond(load, Answer::empty());
            }
            Verdict::Serve { folder } => {
                if let Some(document) = &self.0.open.document {
                    let answer = document.answer(url.path(), !folder);
                    respond(load, answer);
                    return;
                }
                let root = self.0.open.root.as_deref().filter(|_| folder);
                let path = url.path().to_owned();
                self.0.preview.handle(root, &path, self.0.asked.wait(load));
                // What the preview answered without its worker.
                self.0.asked.settle(respond);
            }
        }
    }

    /// A `<select>` gets its list from the window; every other control a
    /// page asks for is closed at once, so no page waits for one.
    fn show_embedder_control(&self, _: WebView, control: EmbedderControl) {
        if let Some(kind) = self.0.held.show(control) {
            self.0.log(false, |skipped| {
                tracing::debug!(kind, skipped, "Closed a control a page asked for");
            });
        }
    }

    fn hide_embedder_control(&self, _: WebView, id: EmbedderControlId) {
        self.0.held.hide(id);
    }

    /// The page's console and its uncaught script errors, for whoever asks
    /// why a page stays empty. Untrusted text: logged as a payload within
    /// the tab's budget, without the query and fragment of any address in
    /// it, and never read.
    fn show_console_message(&self, _: WebView, level: servo::ConsoleLogLevel, message: String) {
        use servo::ConsoleLogLevel::{Error, Warn};
        if !self.0.open.log_console {
            return;
        }
        self.0.log(matches!(level, Error), |skipped| {
            let (text, cut) = console::bounded(&message);
            let text = console::without_queries(text);
            let text = &*text;
            if matches!(level, Error | Warn) {
                tracing::info!(?level, cut, skipped, text, "Page console");
            } else {
                tracing::debug!(?level, cut, skipped, text, "Page console");
            }
        });
    }
}

struct Page {
    view: WebView,
    context: Rc<SoftwareRenderingContext>,
    state: Rc<State>,
}

struct Host {
    servo: Servo,
    pages: HashMap<PageKey, Page>,
    preview: Option<Rc<Preview>>,
    asked: Rc<Asked>,
    theme: String,
}

impl Host {
    fn new(waker: Wake, answers: Sender<(u64, Answer)>) -> Self {
        let asked = Rc::new(Asked::new(answers, waker.clone()));
        let mut registry = ProtocolRegistry::default();
        if let Err(error) = registry.register(PREVIEW_SCHEME, NoPage) {
            tracing::warn!(?error, "Servo refused the preview scheme");
        }
        // Where Servo keeps cookies and site storage between runs; without
        // a folder it keeps them for this run only.
        let opts = Opts {
            config_dir: profile::open(),
            ..Opts::default()
        };
        let servo = ServoBuilder::default()
            .opts(opts)
            .preferences(preferences())
            .event_loop_waker(Box::new(waker))
            .protocol_registry(registry)
            .build();
        servo.set_delegate(Rc::new(AppDelegate));
        Self {
            servo,
            pages: HashMap::new(),
            preview: None,
            asked,
            theme: String::new(),
        }
    }

    /// Reads local pages' files; started with the first page.
    fn preview(&mut self) -> std::io::Result<Rc<Preview>> {
        if let Some(preview) = &self.preview {
            return Ok(preview.clone());
        }
        let preview = Rc::new(Preview::start()?);
        preview.set_theme(self.theme.clone());
        self.preview = Some(preview.clone());
        Ok(preview)
    }

    fn open(&mut self, open: Open) -> Result<(), Error> {
        let key = open.key;
        let url = url::Url::parse(&open.url).map_err(|_| Error::Address)?;
        let preview = self
            .preview()
            .map_err(|error| Error::Thread(Arc::new(error)))?;
        let context = Rc::new(
            SoftwareRenderingContext::new(FIRST_SIZE)
                .map_err(|error| Error::Surface(format!("{error:?}")))?,
        );
        let scripts = Rc::new(UserContentManager::new(&self.servo));
        scripts.add_script(Rc::new(UserScript::new(IPC.to_owned(), None)));
        let probe = crate::browser::blank::PROBE.to_owned();
        scripts.add_script(Rc::new(UserScript::new(probe, None)));
        if open.bridge {
            let bridge = crate::browser::annotate::BRIDGE.to_owned();
            scripts.add_script(Rc::new(UserScript::new(bridge, None)));
        }
        let light = open.light;
        let held = Held::new(open.mailbox.clone(), open.notifier.clone());
        let state = Rc::new(State {
            open,
            preview,
            asked: self.asked.clone(),
            frame_ready: Cell::new(false),
            shown: Cell::new(false),
            scale: Cell::new(1.),
            top: RefCell::new(None),
            budget: RefCell::default(),
            held,
        });
        let view = WebViewBuilder::new(&self.servo, context.clone())
            .delegate(Rc::new(Delegate(state.clone())))
            .user_content_manager(scripts)
            .url(url)
            .build();
        // Hidden until its window presents it.
        view.hide();
        if let Some(light) = light {
            view.notify_theme_change(theme(light));
        }
        self.pages.insert(
            key,
            Page {
                view,
                context,
                state,
            },
        );
        Ok(())
    }

    /// Applies one command. Returns whether the engine goes on.
    fn apply(&mut self, command: Command) -> bool {
        match command {
            Command::Open(open) => {
                let (source, reports) = (open.source, open.reports.clone());
                if let Err(error) = self.open(*open) {
                    tracing::warn!(%error, "Cannot create a Servo page");
                    report(&reports, Event::Failed(source, error));
                }
            }
            // Dropping the last handle closes the view in Servo.
            Command::Close(key) => drop(self.pages.remove(&key)),
            Command::Load(key, url) => {
                if let (Some(page), Ok(url)) = (self.pages.get(&key), url::Url::parse(&url)) {
                    page.state.held.dismiss();
                    page.state.budget.borrow_mut().renew();
                    page.view.load(url);
                }
            }
            // Servo reports no start of a load for a reload it is asked
            // for, so the budget is renewed here, with the command.
            Command::Reload(key) => self.asked_for(key, WebView::reload),
            Command::History(key, forward) => self.asked_for(key, |view| {
                if forward && view.can_go_forward() {
                    view.go_forward(1);
                } else if !forward && view.can_go_back() {
                    view.go_back(1);
                }
            }),
            Command::Sized(key) => {
                if let Some(page) = self.pages.get(&key)
                    && let Some((width, height, scale)) = page.state.open.latest.take_sized()
                {
                    page.state.scale.set(scale);
                    page.view.set_hidpi_scale_factor(euclid::Scale::new(scale));
                    page.view.resize(dpi::PhysicalSize::new(width, height));
                }
            }
            // Empty when the window sent the place itself, ahead of a press.
            Command::Moved(key) => {
                if let Some(page) = self.pages.get(&key)
                    && let Some((x, y)) = page.state.open.latest.take_moved()
                {
                    let point = WebViewPoint::Device(DevicePoint::new(x, y));
                    page.view
                        .notify_input_event(InputEvent::MouseMove(MouseMoveEvent::new(point)));
                }
            }
            Command::Shown(key, shown) => {
                if let Some(page) = self.pages.get(&key) {
                    page.state.shown.set(shown);
                    if shown {
                        page.view.show();
                        // What it drew while hidden was never handed over.
                        page.state.frame_ready.set(true);
                    } else {
                        page.state.held.dismiss();
                        page.view.hide();
                    }
                }
            }
            Command::Focused(key, focused) => {
                if let Some(page) = self.pages.get(&key) {
                    if !focused {
                        page.state.held.dismiss();
                    }
                    page.view.set_focused(focused);
                }
            }
            Command::Chosen(key, number, choice) => {
                if let Some(page) = self.pages.get(&key) {
                    page.state.held.answer(number, choice);
                }
            }
            Command::Zoom(key, zoom) => self.with(key, |view| view.set_page_zoom(zoom)),
            Command::Light(key, light) => {
                self.with(key, |view| view.notify_theme_change(theme(light)));
            }
            Command::Input(key, event) => self.with(key, |view| {
                view.notify_input_event(*event);
            }),
            Command::Script(key, script) => {
                self.with(key, |view| view.evaluate_javascript(script, |_| {}));
            }
            Command::Evaluate(key, script, answered) => self.evaluate(key, script, answered),
            Command::Forget(done) => {
                self.forget();
                // The asker may have stopped waiting.
                let _ = done.try_send(());
            }
            Command::Theme(css) => {
                if let Some(preview) = &self.preview {
                    preview.set_theme(css.clone());
                }
                self.theme = css;
            }
            Command::Quit => return false,
        }
        true
    }

    /// Drops what Servo holds of every site in memory: cookies, the cached
    /// answers, and the `localStorage` and `sessionStorage` of the sites
    /// Servo lists (it lists none for an address that is a bare host name
    /// or a number). The folder on disk is the caller's to empty.
    fn forget(&self) {
        let sites = self.servo.site_data_manager();
        sites.clear_cookies(None);
        let stored = StorageType::Local | StorageType::Session;
        let names: Vec<String> = sites.site_data(stored).iter().map(SiteData::name).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        sites.clear_site_data(&names, stored);
        self.servo.network_manager().clear_cache();
        tracing::info!("Cleared the cookies and site storage of browser tabs");
    }

    /// Gives the request `number` names the file the preview's worker read.
    fn answer(&self, number: u64, answer: Answer) {
        if let Some(load) = self.asked.take(number) {
            respond(load, answer);
        }
    }

    fn with(&self, key: PageKey, act: impl FnOnce(&WebView)) {
        if let Some(page) = self.pages.get(&key) {
            act(&page.view);
        }
    }

    /// A load the user asks for: the tab gets its whole log budget again,
    /// so "reload and read the log" shows the page's lines however much it
    /// wrote before. A load the page starts itself renews nothing.
    fn asked_for(&self, key: PageKey, load: impl FnOnce(&WebView)) {
        if let Some(page) = self.pages.get(&key) {
            page.state.held.dismiss();
            page.state.budget.borrow_mut().renew();
            load(&page.view);
        }
    }

    fn evaluate(&self, key: PageKey, script: String, answered: Answered) {
        let Some(page) = self.pages.get(&key) else {
            answered("null".to_owned());
            return;
        };
        page.view.evaluate_javascript(script, move |result| {
            let value = result.map_or(serde_json::Value::Null, json);
            answered(value.to_string());
        });
    }

    /// Paints each shown page Servo has a new frame for, and hands the
    /// frame to its window.
    fn paint(&self) {
        for page in self.pages.values() {
            if !page.state.shown.get() || !page.state.frame_ready.replace(false) {
                continue;
            }
            page.view.paint();
            let size = page.context.size();
            let (Ok(width), Ok(height)) = (i32::try_from(size.width), i32::try_from(size.height))
            else {
                continue;
            };
            let rect = DeviceIntRect::from_origin_and_size(
                DeviceIntPoint::new(0, 0),
                DeviceIntSize::new(width, height),
            );
            // Read from the buffer Servo drew into, and never present it:
            // nothing shows this context, and presenting swaps in a buffer
            // that holds an older frame, which a resize then handed over.
            let Some(image) = page.context.read_to_image(rect) else {
                continue;
            };
            let (width, height) = image.dimensions();
            let mut bgra = image.into_raw();
            for pixel in bgra.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            page.state.open.mailbox.put(Frame {
                width,
                height,
                scale: page.state.scale.get(),
                bgra,
            });
            page.state.open.notifier.notify();
        }
    }
}

fn theme(light: bool) -> Theme {
    if light { Theme::Light } else { Theme::Dark }
}

/// A script's answer as JSON. Nodes and windows have none.
fn json(value: JSValue) -> serde_json::Value {
    use serde_json::Value;
    match value {
        JSValue::Boolean(value) => Value::Bool(value),
        JSValue::Number(value) => {
            serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
        }
        JSValue::String(value) => Value::String(value),
        JSValue::Array(values) => Value::Array(values.into_iter().map(json).collect()),
        JSValue::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(name, value)| (name, json(value)))
                .collect(),
        ),
        JSValue::Undefined
        | JSValue::Null
        | JSValue::Element(_)
        | JSValue::ShadowRoot(_)
        | JSValue::Frame(_)
        | JSValue::Window(_) => Value::Null,
    }
}

/// Runs the engine until the app quits or its last handle is gone.
/// `controls` are read before `inputs`, so a page exists before its input
/// arrives and closes however much input waits.
pub(super) fn run(
    controls: &Receiver<Command>,
    inputs: &Receiver<Command>,
    woken: &Receiver<()>,
    waker: Wake,
) {
    let (answers, answered) = crossbeam_channel::bounded(ANSWERS);
    let mut host = Host::new(waker, answers);
    'engine: loop {
        let mut input = None;
        select! {
            recv(answered) -> answer => {
                if let Ok((number, answer)) = answer {
                    host.answer(number, answer);
                }
            },
            recv(controls) -> command => match command {
                Ok(command) => {
                    if !host.apply(command) {
                        break 'engine;
                    }
                }
                Err(_) => break 'engine,
            },
            recv(inputs) -> command => match command {
                Ok(command) => input = Some(command),
                Err(_) => break 'engine,
            },
            recv(woken) -> woken => {
                if woken.is_err() {
                    break 'engine;
                }
            }
        }
        for (number, answer) in answered.try_iter().take(BATCH) {
            host.answer(number, answer);
        }
        for command in controls.try_iter().take(BATCH) {
            if !host.apply(command) {
                break 'engine;
            }
        }
        for command in input.into_iter().chain(inputs.try_iter().take(BATCH)) {
            host.apply(command);
        }
        host.servo.spin_event_loop();
        host.paint();
    }
    // The preview's worker may wait to hand an answer over: it stops waiting
    // once nothing reads them.
    drop(answered);
    // Servo's shutdown waits for its threads: here, never on the UI thread.
    host.pages.clear();
    drop(host);
}

#[cfg(test)]
mod tests;
