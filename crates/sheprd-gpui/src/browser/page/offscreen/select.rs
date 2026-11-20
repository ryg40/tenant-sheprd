//! The list of a page's open `<select>`. Servo draws the control and leaves
//! its list to the host, so the window draws it over the page, at the
//! control, in the app's theme. The page's text is only shown here, never
//! read, and arrives bounded ([`crate::browser::engine::controls`]).
//!
//! The list takes the page's keys while it is open and the keyboard stays
//! with the page: the page hears neither such a key nor its release. Tab
//! closes the list and goes on to the page, which moves its focus. Every
//! way the list closes answers the engine: a choice, Escape, a press
//! outside it, a wheel turn over the page, a new size or zoom, and the page
//! hiding or losing the keyboard.
use super::Page;
use crate::{
    browser::engine::{
        self, Command,
        controls::{Menu, Row},
    },
    config::{Theme, corners},
};
use gpui::{
    Bounds, Div, InteractiveElement, IntoElement, KeyDownEvent, KeyUpEvent, Keystroke, MouseButton,
    ParentElement, Pixels, Point, ScrollStrategy, Size, Stateful, StatefulInteractiveElement,
    Styled, UniformListScrollHandle, Window, deferred, div, point, prelude::FluentBuilder as _, px,
    rgb, size, uniform_list,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

/// The height of one row, which every row has.
const ROW: f32 = 24.;
/// Space between the list's edge and its rows, with the border.
const INSET: f32 = 5.;
const MAX_HEIGHT: f32 = 320.;
const MIN_WIDTH: f32 = 160.;
const MAX_WIDTH: f32 = 480.;
/// The page's edge the list keeps away from.
const MARGIN: f32 = 4.;
/// Rows Page Up and Page Down move by.
const PAGE: usize = 10;
/// How long typed letters stay one word to find an option by.
const TYPING: Duration = Duration::from_secs(1);
/// Letters of that word that count.
const TYPED: usize = 64;
/// How far under a press the list opens when the control's place is not
/// known.
const UNDER_PRESS: f32 = 10.;
/// Keys held at once that the list remembers it took.
const TAKEN: usize = 16;

/// The theme's colors a list is drawn in.
#[derive(Clone, Copy)]
pub(crate) struct Colors {
    surface: u32,
    active: u32,
    foreground: u32,
    muted: u32,
}

impl From<&Theme> for Colors {
    fn from(theme: &Theme) -> Self {
        Self {
            surface: theme.surface,
            active: theme.active,
            foreground: theme.foreground,
            muted: theme.muted,
        }
    }
}

impl Default for Colors {
    fn default() -> Self {
        Self::from(&Theme::default())
    }
}

/// What the list's page is told: the options chosen, or none when the list
/// closed without a choice.
type Choice = Option<Vec<usize>>;

/// Whether the list stays after a key or a press.
#[derive(Debug, Eq, PartialEq)]
enum Outcome {
    Open,
    Closed(Choice),
}

/// One open list.
struct List {
    menu: Arc<Menu>,
    /// The row the keys are on.
    at: Option<usize>,
    /// The options chosen so far, by Servo's names for them.
    picked: Vec<usize>,
    scroll: UniformListScrollHandle,
    /// Letters typed to find an option, and when the last one was.
    typed: String,
    typed_at: Option<Instant>,
}

impl List {
    fn new(menu: Arc<Menu>) -> Self {
        let picked = menu.chosen.clone();
        let at = picked.first().and_then(|chosen| {
            menu.rows
                .iter()
                .position(|row| matches!(row, Row::Choice { id, .. } if id == chosen))
        });
        let scroll = UniformListScrollHandle::default();
        if let Some(at) = at {
            scroll.scroll_to_item(at, ScrollStrategy::Center);
        }
        Self {
            menu,
            at,
            picked,
            scroll,
            typed: String::new(),
            typed_at: None,
        }
    }

    /// The option at `row`, if a person may choose it.
    fn option(&self, row: usize) -> Option<(usize, &str)> {
        match self.menu.rows.get(row)? {
            Row::Choice {
                id,
                label,
                disabled: false,
                ..
            } => Some((*id, label)),
            Row::Choice { .. } | Row::Group(_) => None,
        }
    }

    /// The row `steps` options before or after the keys' row, or the last
    /// one that way.
    fn step(&self, forward: bool, steps: usize) -> Option<usize> {
        let count = self.menu.rows.len();
        let open = |row: &usize| self.option(*row).is_some();
        if forward {
            let start = self.at.map_or(0, |at| at + 1);
            (start..count).filter(open).take(steps).last()
        } else {
            let end = self.at.unwrap_or(count);
            (0..end).rev().filter(open).take(steps).last()
        }
    }

    /// Moves the keys to `row`, if there is one to move to.
    fn go(&mut self, row: Option<usize>) {
        if let Some(row) = row {
            self.at = Some(row);
            self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
        }
    }

    /// A press or Enter on `row`. One choice closes the list; a list of
    /// several keeps it and stays open.
    fn choose(&mut self, row: usize) -> Outcome {
        let Some((id, _)) = self.option(row) else {
            return Outcome::Open;
        };
        self.at = Some(row);
        if !self.menu.multiple {
            return Outcome::Closed(Some(vec![id]));
        }
        match self.picked.iter().position(|picked| *picked == id) {
            Some(place) => {
                self.picked.remove(place);
            }
            None => self.picked.push(id),
        }
        Outcome::Open
    }

    /// What closing the list by leaving it answers: a list of several keeps
    /// what was chosen in it, and a list of one changes nothing.
    fn leave(&self) -> Choice {
        self.menu.multiple.then(|| self.picked.clone())
    }

    fn typing(&self, now: Instant) -> bool {
        !self.typed.is_empty()
            && self
                .typed_at
                .is_some_and(|at| now.saturating_duration_since(at) <= TYPING)
    }

    /// Moves the keys to the next option that starts with what was typed.
    /// One letter typed again goes on to the next such option.
    fn find(&mut self, typed: &str, now: Instant) {
        if !self.typing(now) {
            self.typed.clear();
        }
        self.typed_at = Some(now);
        let room = TYPED.saturating_sub(self.typed.chars().count());
        self.typed.extend(
            typed
                .chars()
                .filter(|c| !c.is_control())
                .flat_map(char::to_lowercase)
                .take(room),
        );
        if self.typed.is_empty() {
            return;
        }
        let count = self.menu.rows.len();
        let first = match self.at {
            Some(at) if self.typed.chars().count() == 1 => at + 1,
            Some(at) => at,
            None => 0,
        };
        let found = (0..count).map(|ahead| (first + ahead) % count).find(|row| {
            self.option(*row)
                .is_some_and(|(_, label)| label.to_lowercase().starts_with(&self.typed))
        });
        self.go(found);
    }

    fn key(&mut self, keystroke: &Keystroke, now: Instant) -> Outcome {
        let held = keystroke.modifiers;
        match keystroke.key.as_str() {
            "escape" => return Outcome::Closed(None),
            "tab" => return Outcome::Closed(self.leave()),
            "enter" if self.menu.multiple => return Outcome::Closed(self.leave()),
            "enter" => {
                return self
                    .at
                    .map_or(Outcome::Closed(None), |row| self.choose(row));
            }
            // A space belongs to the word being typed, as in "New York".
            "space" if !self.typing(now) => {
                if let Some(row) = self.at {
                    return self.choose(row);
                }
            }
            "down" => self.go(self.step(true, 1)),
            "up" => self.go(self.step(false, 1)),
            "pagedown" => self.go(self.step(true, PAGE)),
            "pageup" => self.go(self.step(false, PAGE)),
            "home" => {
                self.at = None;
                self.go(self.step(true, 1));
            }
            "end" => {
                self.at = None;
                self.go(self.step(false, 1));
            }
            _ => {
                if !held.control
                    && !held.alt
                    && !held.platform
                    && let Some(typed) = keystroke.key_char.as_deref()
                {
                    self.find(typed, now);
                }
            }
        }
        Outcome::Open
    }
}

/// An answer and the list it is for.
type Answer = (u64, Choice);

/// The answer the engine has not taken yet. Its queue was full, which a
/// living engine's is not: the answer waits here and is sent again, so the
/// page's request is not left open.
#[derive(Default)]
struct Owed(RefCell<Option<Answer>>);

impl Owed {
    /// Sends `answer`, and keeps it when `send` could not. An older answer
    /// still owed is for a list the engine has closed since.
    fn give(&self, answer: Answer, send: impl FnOnce(&Answer) -> bool) {
        *self.0.borrow_mut() = Some(answer);
        self.settle(send);
    }

    /// Sends the answer that waits, if one does.
    fn settle(&self, send: impl FnOnce(&Answer) -> bool) {
        let owed = self.0.borrow_mut().take();
        if let Some(answer) = owed
            && !send(&answer)
        {
            *self.0.borrow_mut() = Some(answer);
        }
    }
}

/// The keys the list took and that are still down. The page did not hear
/// them go down, so it hears neither their repeats nor their release, also
/// after the list closed.
#[derive(Default)]
struct Taken(RefCell<Vec<String>>);

impl Taken {
    /// Notes that the list took `key`.
    fn took(&self, key: &str) {
        let mut taken = self.0.borrow_mut();
        if taken.iter().any(|taken| taken == key) {
            return;
        }
        if taken.len() == TAKEN {
            taken.remove(0);
        }
        taken.push(key.to_owned());
    }

    /// Whether `key`, pressed with no list open, is a repeat of a key the
    /// list took. A new press is the page's, whatever is remembered.
    fn repeats(&self, key: &str, held: bool) -> bool {
        if held {
            return self.0.borrow().iter().any(|taken| taken == key);
        }
        self.released(key);
        false
    }

    /// Whether the release of `key` ends a press the list took.
    fn released(&self, key: &str) -> bool {
        let mut taken = self.0.borrow_mut();
        let before = taken.len();
        taken.retain(|taken| taken != key);
        taken.len() != before
    }
}

/// A page's list and what the window remembers around it.
#[derive(Default)]
pub(super) struct Select {
    list: RefCell<Option<List>>,
    /// The latest list this window closed, which the engine may still show
    /// as open until it reads the answer.
    answered: Cell<u64>,
    owed: Owed,
    /// Where the page was last pressed, in its own points, until a key
    /// follows: a list that opens then opened from that press.
    press: Cell<Option<Point<Pixels>>>,
    /// The press that closed the list from outside it, which the page does
    /// not hear, and the button whose release it does not hear either.
    closed_by: Cell<Option<Point<Pixels>>>,
    release: Cell<Option<MouseButton>>,
    taken: Taken,
}

impl Page {
    /// Hands the engine an answer. False only when the answer must be sent
    /// again: an engine that is gone holds no request to answer.
    fn deliver(&self, (number, choice): &Answer) -> bool {
        let chosen = Command::Chosen(self.key, *number, choice.clone());
        !matches!(self.engine.send(chosen), Err(engine::Error::Busy))
    }

    /// Sends the answer the engine did not take, if one waits.
    pub(super) fn settle_list(&self) {
        self.select.owed.settle(|answer| self.deliver(answer));
    }

    /// Closes the list and answers the page with `choice`.
    fn answer_list(&self, choice: Choice) {
        let Some(list) = self.select.list.take() else {
            return;
        };
        let number = list.menu.number;
        self.select.answered.set(number);
        self.select
            .owed
            .give((number, choice), |answer| self.deliver(answer));
    }

    /// Closes the list without a choice, if one is open.
    pub(super) fn dismiss_list(&self) {
        self.answer_list(None);
    }

    /// Whether a list of the page is open.
    pub(super) fn lists(&self) -> bool {
        self.select.list.borrow().is_some()
    }

    /// Shows the list the engine has for the page, and closes one it no
    /// longer has. Cheap: called as the page lays out.
    fn follow_list(&self) {
        let listed = self.mailbox.list.get();
        let mut list = self.select.list.borrow_mut();
        match listed {
            None => *list = None,
            Some(menu) if menu.number <= self.select.answered.get() => {}
            Some(menu) => {
                if list
                    .as_ref()
                    .is_none_or(|list| list.menu.number != menu.number)
                {
                    *list = Some(List::new(menu));
                }
            }
        }
    }

    fn act(&self, outcome: Option<Outcome>, window: &mut Window) {
        if let Some(Outcome::Closed(choice)) = outcome {
            self.answer_list(choice);
        }
        window.refresh();
    }

    /// Hands a key to the open list. Whether the list took it, and the
    /// page hears neither it nor its release. Tab closes the list and is
    /// the page's too: its focus moves on, as without a list.
    pub(super) fn list_key(&self, event: &KeyDownEvent, window: &mut Window) -> bool {
        let key = event.keystroke.key.as_str();
        let outcome = self
            .select
            .list
            .borrow_mut()
            .as_mut()
            .map(|list| list.key(&event.keystroke, Instant::now()));
        if outcome.is_none() {
            return self.select.taken.repeats(key, event.is_held);
        }
        self.act(outcome, window);
        if key == "tab" {
            return false;
        }
        // The page heard a key go down that was held as the list opened,
        // and hears its release.
        if !event.is_held {
            self.select.taken.took(key);
        }
        true
    }

    /// Whether the release of a key ends a press the list took.
    pub(super) fn list_key_up(&self, event: &KeyUpEvent) -> bool {
        self.select.taken.released(&event.keystroke.key)
    }

    fn list_press(&self, row: usize, window: &mut Window) {
        let outcome = self
            .select
            .list
            .borrow_mut()
            .as_mut()
            .map(|list| list.choose(row));
        self.act(outcome, window);
    }

    /// Notes a press the page heard, at `place` in the page's points.
    pub(super) fn pressed_at(&self, place: Point<Pixels>) {
        self.select.press.set(Some(place));
    }

    /// Notes a key the page heard.
    pub(super) fn keyed(&self) {
        self.select.press.set(None);
    }

    /// Whether the press at `position` just closed the list from outside
    /// it. The page then hears neither it nor the release of `button`: a
    /// press on the control closes its list rather than open it again.
    pub(super) fn closed_list(&self, position: Point<Pixels>, button: MouseButton) -> bool {
        let closed = self.select.closed_by.take() == Some(position);
        if closed {
            self.select.release.set(Some(button));
        }
        closed
    }

    /// Whether the release of `button` ends a press the page did not hear.
    pub(super) fn released_list(&self, button: MouseButton) -> bool {
        if self.select.release.get() == Some(button) {
            self.select.release.set(None);
            return true;
        }
        false
    }

    /// The page's size in points, and its device pixels to a point, as the
    /// engine was last told.
    fn points(&self) -> Option<(Size<Pixels>, f32)> {
        let (width, height, scale) = self.sized.get()?;
        (scale.is_finite() && scale > 0.).then(|| {
            let points = size(px(width as f32 / scale), px(height as f32 / scale));
            (points, scale)
        })
    }
}

/// Where the list's control is, in the points of a page of `scale` device
/// pixels to a point. Servo reports the control's box in the page's device
/// pixels, with the page's zoom, its scroll, and the frames around the
/// control applied, and cut to the page. A box with no size is one Servo
/// could not place: the list then opens under the press that opened it.
/// A box with a size is used as it is, also far from that press: a page
/// can move the control during the press. Servo does not say when it
/// placed a box with a part of its frames only, so such a box is not
/// told apart.
fn control(menu: &Menu, scale: f32, press: Option<Point<Pixels>>) -> Bounds<Pixels> {
    #[expect(clippy::cast_precision_loss, reason = "a place on a page is small")]
    let [left, top, width, height] = menu.control.map(|device| device as f32 / scale);
    let reported = Bounds::new(
        point(px(left), px(top)),
        size(px(width.max(0.)), px(height.max(0.))),
    );
    match press {
        Some(press) if reported.size.width <= px(0.) || reported.size.height <= px(0.) => {
            Bounds::new(
                point(press.x, press.y + px(UNDER_PRESS)),
                size(px(0.), px(0.)),
            )
        }
        _ => reported,
    }
}

/// Where a list of `rows` rows draws for `control`, both in the points of a
/// page of size `page`: under the control, or over it when more of the list
/// fits there, and inside the page whatever the page reported.
fn place(control: Bounds<Pixels>, page: Size<Pixels>, rows: usize) -> Bounds<Pixels> {
    let (page_width, page_height) = (f32::from(page.width), f32::from(page.height));
    let top = f32::from(control.top()).max(0.).min(page_height);
    let bottom = f32::from(control.bottom()).max(top).min(page_height);
    #[expect(clippy::cast_precision_loss, reason = "rows are bounded")]
    let wanted = (rows as f32 * ROW + 2. * INSET).min(MAX_HEIGHT);
    let below = (page_height - bottom - MARGIN).max(0.);
    let above = (top - MARGIN).max(0.);
    let (y, height) = if wanted <= below || below >= above {
        (bottom, wanted.min(below))
    } else {
        let height = wanted.min(above);
        (top - height, height)
    };
    let width = f32::from(control.size.width)
        .clamp(MIN_WIDTH, MAX_WIDTH)
        .min((page_width - 2. * MARGIN).max(0.));
    let x = f32::from(control.left())
        .min(page_width - MARGIN - width)
        .max(MARGIN);
    Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
}

fn row(page: &Rc<Page>, list: &List, index: usize, colors: Colors) -> Stateful<Div> {
    let line = div()
        .id(("page-select-row", index))
        .w_full()
        .h(px(ROW))
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(6.))
        .rounded(px(corners::SMALL))
        .whitespace_nowrap();
    let (id, label, disabled, grouped) = match list.menu.rows.get(index) {
        Some(Row::Choice {
            id,
            label,
            disabled,
            grouped,
        }) => (*id, label, *disabled, *grouped),
        Some(Row::Group(label)) => {
            return line
                .text_color(rgb(colors.muted))
                .child(div().min_w_0().truncate().child(label.clone()));
        }
        None => return line,
    };
    // i18n: data
    let mark = if list.picked.contains(&id) { "✓" } else { "" };
    let line = line
        .when(grouped, |line| line.pl(px(18.)))
        .child(div().flex_none().w(px(12.)).child(mark))
        .child(div().min_w_0().truncate().child(label.clone()));
    if disabled {
        return line.text_color(rgb(colors.muted)).opacity(0.6);
    }
    let page = page.clone();
    line.cursor_pointer()
        .when(list.at == Some(index), |line| line.bg(rgb(colors.active)))
        .hover(|line| line.bg(rgb(colors.active)))
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            page.list_press(index, window);
        })
}

/// The page's open list as an element of the page's own, drawn over the
/// page, if the page has one.
pub(super) fn element(page: &Rc<Page>, colors: Colors) -> Option<impl IntoElement + use<>> {
    page.follow_list();
    let (number, count, bounds, scroll) = {
        let list = page.select.list.borrow();
        let list = list.as_ref()?;
        let (points, scale) = page.points()?;
        let control = control(&list.menu, scale, page.select.press.get());
        let count = list.menu.rows.len();
        (
            list.menu.number,
            count,
            place(control, points, count),
            list.scroll.clone(),
        )
    };
    let rows = {
        let page = page.clone();
        uniform_list("page-select-rows", count, move |range, _, _| {
            let list = page.select.list.borrow();
            let Some(list) = list.as_ref() else {
                return Vec::new();
            };
            range.map(|index| row(&page, list, index, colors)).collect()
        })
        .track_scroll(&scroll)
        .size_full()
    };
    let outside = page.clone();
    let list = div()
        .id(("page-select", page.key))
        // i18n: data
        .debug_selector(|| "page-select".into())
        .absolute()
        .left(bounds.origin.x)
        .top(bounds.origin.y)
        .w(bounds.size.width)
        .h(bounds.size.height)
        .p(px(INSET - 1.))
        .bg(rgb(colors.surface))
        .border_1()
        .border_color(rgb(colors.active))
        .rounded(px(corners::CONTROL))
        .shadow_md()
        .text_color(rgb(colors.foreground))
        .occlude()
        .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
        .on_mouse_down_out(move |event, window, _| {
            let choice = outside.select.list.borrow().as_ref().map(List::leave);
            if let Some(choice) = choice {
                outside.select.closed_by.set(Some(event.position));
                outside.answer_list(choice);
                window.refresh();
            }
        })
        .child(rows);
    Some(deferred(crate::motion::pop_in(list, ("page-select-in", number))).with_priority(1))
}

#[cfg(test)]
mod tests;
