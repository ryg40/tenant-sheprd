//! The hint a tab shows when a site sent a page with nothing in it, as a
//! sign-in's last address does when it refuses its code: an error status
//! with no body. Servo draws nothing for such an answer and tells its host
//! no status, so the tab would stay empty without a word.
//!
//! The window learns it from the document, with a script of the app's own
//! that the engine runs before the page's scripts, and never asks the site
//! again: such an address often holds a code that works once. A page may fill itself by script after it loads,
//! so the page is asked only a short time after its load, while it is
//! shown, and asked again while the hint shows, which takes the hint away
//! once the page has content.
use super::TabId;
use crate::{SheprdWindow, browser::groups::Slot, i18n::fl};
use gpui::{prelude::*, *};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

/// How long a page has after its load to fill itself by script before it
/// is asked whether it is empty.
const SETTLE: Duration = Duration::from_millis(700);

/// How often a page that showed the hint is asked again.
const AGAIN: Duration = Duration::from_secs(2);

/// Runs in every document before the page's own scripts (a user script of
/// the engine), and makes the question [`SCRIPT`] asks. It keeps the
/// engine's own accessors from before the page ran, and a page can neither
/// replace nor shadow a `const` of the global scope, so the answer is the
/// engine's and not one the page chose.
///
/// The answer says whether the document has nothing to show: `true` or
/// `false`, and `null` while it still loads. Any element in the body counts
/// as content, drawn or not, and so does a document that is not HTML (an
/// image or an SVG gets no user script, and so no question to answer).
/// Nothing of the page is run or read back.
// i18n: data
pub(in crate::browser) const PROBE: &str = "const __sheprdBlank = (function () {\
  var apply = Reflect.apply;\
  var getter = function (proto, name) {\
    return Object.getOwnPropertyDescriptor(proto, name).get;\
  };\
  var doc = document;\
  var ready = getter(Document.prototype, 'readyState');\
  var root = getter(Document.prototype, 'documentElement');\
  var body = getter(Document.prototype, 'body');\
  var space = getter(Element.prototype, 'namespaceURI');\
  var local = getter(Element.prototype, 'localName');\
  var count = getter(Element.prototype, 'childElementCount');\
  var text = getter(Node.prototype, 'textContent');\
  var trim = String.prototype.trim;\
  return function () {\
    if (apply(ready, doc, []) !== 'complete') { return null; }\
    var top = apply(root, doc, []);\
    if (!top) { return true; }\
    if (apply(space, top, []) !== 'http://www.w3.org/1999/xhtml' ||\
        apply(local, top, []) !== 'html') { return false; }\
    var inner = apply(body, doc, []);\
    if (!inner) { return true; }\
    if (apply(count, inner, []) > 0) { return false; }\
    return apply(trim, apply(text, inner, []), []) === '';\
  };\
})();";

/// Asks the document [`PROBE`]'s question. A document without it answers
/// with an error, which the engine reports as `null`.
// i18n: data
const SCRIPT: &str = "__sheprdBlank()";

/// How many times in a row a page may give no answer before it is no
/// longer asked: a document that never finishes, or one the question was
/// not made in, costs a few questions and gets no hint.
const UNANSWERED: u8 = 3;

/// What a page answered, written on the engine's thread.
type Answer = Arc<Mutex<Option<String>>>;

enum Step {
    /// The page is asked once this time has come.
    Wait(Instant),
    Asked(Answer),
}

struct Check {
    /// Whether the tab shows the hint now.
    blank: bool,
    /// The questions in a row that got no answer.
    unanswered: u8,
    step: Step,
}

/// One window's record of which pages finished loading with nothing in
/// them. A tab has an entry from its page's load until the page answers
/// that it has content; bounded by the tabs the store keeps.
#[derive(Default)]
pub(crate) struct Blank {
    checks: HashMap<TabId, Check>,
}

impl Blank {
    /// Tab `id`'s page finished a load at `now`: the hint of the page
    /// before it goes, and the new page is asked after [`SETTLE`]. Whether
    /// a hint went.
    pub(super) fn loaded(&mut self, id: TabId, now: Instant) -> bool {
        let check = Check {
            blank: false,
            unanswered: 0,
            step: Step::Wait(now + SETTLE),
        };
        self.checks
            .insert(id, check)
            .is_some_and(|before| before.blank)
    }

    /// The tabs whose page is to be asked at `now`.
    fn due(&self, now: Instant) -> Vec<TabId> {
        self.checks
            .iter()
            .filter(|(_, check)| matches!(check.step, Step::Wait(at) if at <= now))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Marks tab `id` as asked, and returns what takes its page's answer,
    /// on any thread. An answer to an earlier load's question reaches no
    /// one: each load starts a new record.
    fn ask(&mut self, id: TabId) -> impl Fn(String) + Send + 'static {
        let answer = Answer::default();
        if let Some(check) = self.checks.get_mut(&id) {
            check.step = Step::Asked(answer.clone());
        }
        move |raw| *answer.lock().unwrap_or_else(PoisonError::into_inner) = Some(raw)
    }

    /// Applies the answers that arrived. Whether a hint came or went.
    fn settle(&mut self, now: Instant) -> bool {
        let mut changed = false;
        self.checks.retain(|id, check| {
            let Step::Asked(answer) = &check.step else {
                return true;
            };
            let Some(raw) = answer.lock().unwrap_or_else(PoisonError::into_inner).take() else {
                return true;
            };
            // Only the script's own `true` shows the hint. A page that
            // gives no answer is asked again, a few times; after that its
            // record goes, and with it a hint it showed.
            let silent = raw == "null";
            check.unanswered = if silent { check.unanswered + 1 } else { 0 };
            let blank = match raw.as_str() {
                "true" => true,
                "null" => check.blank && check.unanswered < UNANSWERED,
                _ => false,
            };
            changed |= blank != check.blank;
            // The address may hold a sign-in code: only the tab is named.
            if blank && !check.blank {
                tracing::info!(tab = %id, "A site's page loaded with nothing in it");
            } else if check.blank && !blank {
                tracing::debug!(tab = %id, "A page that was empty has content now");
            }
            check.blank = blank;
            check.step = Step::Wait(now + if blank { AGAIN } else { SETTLE });
            blank || (silent && check.unanswered < UNANSWERED)
        });
        changed
    }

    /// Has tab `id`'s page asked [`SETTLE`] after `now` instead of now.
    fn postpone(&mut self, id: TabId, now: Instant) {
        if let Some(check) = self.checks.get_mut(&id) {
            check.step = Step::Wait(now + SETTLE);
        }
    }

    /// Drops tab `id`'s record, and with it its hint. Whether it had one.
    pub(super) fn forget(&mut self, id: TabId) -> bool {
        self.checks.remove(&id).is_some_and(|check| check.blank)
    }

    /// Drops the records of tabs the app no longer has.
    pub(super) fn retain(&mut self, mut live: impl FnMut(TabId) -> bool) {
        self.checks.retain(|id, _| live(*id));
    }

    /// Answers for tab `id`'s page, as the engine's thread does. Whether
    /// the page was asked and had not answered.
    #[cfg(test)]
    pub(super) fn answer(&self, id: TabId, raw: &str) -> bool {
        let Some(Step::Asked(answer)) = self.checks.get(&id).map(|check| &check.step) else {
            return false;
        };
        answer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(raw.to_owned())
            .is_none()
    }

    /// Whether tab `id` shows the hint.
    pub(super) fn shows(&self, id: TabId) -> bool {
        self.checks.get(&id).is_some_and(|check| check.blank)
    }
}

impl SheprdWindow {
    /// Asks the pages that are due whether they are empty, and applies the
    /// answers that arrived. Runs on every window tick; cheap when no page
    /// loaded lately and none shows the hint.
    pub(super) fn poll_blank_pages(&mut self, now: Instant, cx: &mut Context<Self>) {
        if self.browser.blank.checks.is_empty() {
            return;
        }
        let mut changed = self.browser.blank.settle(now);
        for id in self.browser.blank.due(now) {
            // Servo holds the timers of a page that is not shown, so its
            // scripts have not filled it yet: it is asked once it shows.
            if !self.browser.pages.shows(id) {
                self.browser.blank.postpone(id, now);
                continue;
            }
            // Only a site's page gets the hint: a local page is the
            // agent's own file, and an editor draws itself.
            let web = super::view::store(cx)
                .and_then(|store| store.get(id))
                .is_some_and(|tab| {
                    !tab.place.is_editor()
                        && matches!(tab.location, Some(super::Location::Web { .. }))
                });
            let done = self.browser.blank.ask(id);
            // A page the window does not have, or an engine that took no
            // script, never answers: the record goes, and the next load
            // starts another.
            if !web || !matches!(self.browser.pages.evaluate(id, SCRIPT, done), Ok(true)) {
                changed |= self.browser.blank.forget(id);
            }
        }
        if changed {
            cx.notify();
        }
    }

    /// The line a tab shows above a page that has nothing in it, with a
    /// way on. An address with a query may hold a code that works once, as
    /// a sign-in's last address does, and a reload would send it again:
    /// the action then opens the address the tab was asked for on that
    /// site, which starts the sign-in afresh. When the app knows no such
    /// address, the action is a reload and the line says what it sends.
    pub(super) fn blank_hint(&self, slot: Slot, id: TabId, cx: &mut Context<Self>) -> Option<Div> {
        if !self.browser.blank.shows(id) {
            return None;
        }
        let store = super::view::store(cx)?;
        let Some(super::Location::Web { url }) = &store.get(id)?.location else {
            return None;
        };
        let once = url.bare() != *url;
        let asked = store
            .asked_address(id)
            .filter(|asked| once && asked.origin() == url.origin())
            .cloned();
        let theme = &self.theme;
        let action = div()
            .flex_none()
            .max_w(relative(0.5))
            .truncate()
            .px(px(6.))
            .rounded(px(crate::config::corners::CONTROL))
            .text_color(rgb(theme.foreground))
            .cursor_pointer()
            .hover(|button| button.bg(rgb(theme.active)));
        let (text, action) = match asked {
            Some(asked) => (
                fl!("browser-blank-hint"),
                action
                    .id("browser-blank-open")
                    .debug_selector(move || slot.selector("browser-blank-open"))
                    .child(fl!("browser-blank-open", address = asked.as_str()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let location = super::Location::Web { url: asked.clone() };
                        this.browser.blank.forget(id);
                        super::Store::update(cx, |store| store.asked(id, location.clone()));
                        this.browser.pages.load(id, &location);
                        cx.notify();
                    })),
            ),
            None => (
                if once {
                    fl!("browser-blank-hint-resend")
                } else {
                    fl!("browser-blank-hint")
                },
                action
                    .id("browser-blank-reload")
                    .debug_selector(move || slot.selector("browser-blank-reload"))
                    .child(fl!("browser-blank-reload"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.browser.blank.forget(id);
                        this.browser.pages.reload(id);
                        cx.notify();
                    })),
            ),
        };
        Some(
            div()
                .debug_selector(move || slot.selector("browser-blank"))
                .flex()
                .flex_none()
                .items_center()
                .gap(px(8.))
                .px(px(10.))
                .py(px(4.))
                .bg(rgb(theme.surface))
                .border_b_1()
                .border_color(rgb(theme.active))
                .text_color(rgb(theme.muted))
                .child(div().min_w_0().truncate().child(text))
                .child(action),
        )
    }
}

#[cfg(test)]
mod tests;
