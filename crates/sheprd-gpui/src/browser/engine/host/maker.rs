//! How the engine makes a page: its surface, the scripts every page gets,
//! its handlers, and its web view, for a page that a window opens and for
//! the page that Servo builds for a page's new window.
use super::{Asked, Delegate, FIRST_SIZE, Open, Page, PageKey, State};
use crate::browser::{
    engine::{
        clipboard::Clipboard,
        controls::Held,
        cursor::Pointer,
        page_key,
        popup::{self, Activation, Opening},
        stopped::Probe,
    },
    preview::Preview,
    reports::Source,
};
use servo::{
    CreateNewWebViewRequest, RenderingContext, Servo, SoftwareRenderingContext, UserContentManager,
    UserScript, WebViewBuilder,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::Instant,
};

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

/// What every page shares to make a page: Servo, the preview scheme's
/// requests, and the pages that Servo built for new windows since the loop
/// last took them into its own, which it does after each run of Servo.
pub(super) struct Maker {
    pub(super) servo: Servo,
    pub(super) asked: Rc<Asked>,
    pub(super) built: RefCell<Vec<(PageKey, Page)>>,
}

impl Maker {
    /// The page `open` names, at `opening`'s place, drawn on `context`.
    /// `builder` starts its web view on that surface.
    pub(super) fn page(
        self: &Rc<Self>,
        open: Open,
        preview: Rc<Preview>,
        opening: Opening,
        context: Rc<SoftwareRenderingContext>,
        builder: impl FnOnce(Rc<dyn RenderingContext>) -> WebViewBuilder,
    ) -> Page {
        let scripts = Rc::new(UserContentManager::new(&self.servo));
        scripts.add_script(Rc::new(UserScript::new(IPC.to_owned(), None)));
        let probe = crate::browser::blank::PROBE.to_owned();
        scripts.add_script(Rc::new(UserScript::new(probe, None)));
        if open.bridge {
            let bridge = crate::browser::annotate::BRIDGE.to_owned();
            scripts.add_script(Rc::new(UserScript::new(bridge, None)));
        }
        let held = Held::new(open.mailbox.clone(), open.notifier.clone());
        let pointer = Pointer::new(&open);
        let clipboard = Rc::new(Clipboard::new(open.mailbox.clone(), open.notifier.clone()));
        let probe = Probe::new(open.mailbox.clone(), open.notifier.clone());
        let state = Rc::new(State {
            open,
            preview,
            asked: self.asked.clone(),
            frame_ready: Cell::new(false),
            shown: Cell::new(false),
            scale: Cell::new(1.),
            top: RefCell::new(None),
            budget: RefCell::default(),
            pointer,
            held,
            clipboard: clipboard.clone(),
            probe: Rc::new(probe),
            activation: Activation::default(),
            opening,
            maker: self.clone(),
        });
        let surface: Rc<dyn RenderingContext> = context.clone();
        let view = builder(surface)
            .delegate(Rc::new(Delegate(state.clone())))
            .clipboard_delegate(clipboard)
            .user_content_manager(scripts)
            .build();
        // Hidden until its window presents it.
        view.hide();
        Page {
            view,
            context,
            state,
        }
    }

    /// Makes the page of `opener`'s new window from Servo's `request`: a web
    /// page in the opener's window, with no folder and no bridge.
    pub(super) fn popup(
        self: &Rc<Self>,
        opener: &State,
        request: CreateNewWebViewRequest,
    ) -> Result<(), popup::Error> {
        // Dropped, the request answers Servo that no window opens.
        let context =
            SoftwareRenderingContext::new(FIRST_SIZE).map_err(|_| popup::Error::Surface)?;
        let key = page_key();
        let open = Open {
            key,
            url: String::new(),
            root: None,
            document: None,
            bridge: false,
            light: None,
            log_console: opener.open.log_console,
            source: Source::Popup(key),
            reports: opener.open.reports.clone(),
            mailbox: Arc::default(),
            notifier: opener.open.notifier.clone(),
            latest: Arc::default(),
        };
        let opening = Opening::new(opener.open.source, Instant::now());
        let preview = opener.preview.clone();
        let context = Rc::new(context);
        let page = self.page(open, preview, opening, context, |surface| {
            request.builder(surface)
        });
        self.built.borrow_mut().push((key, page));
        Ok(())
    }
}
