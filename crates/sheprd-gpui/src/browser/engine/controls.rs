//! What a page asks its host to show outside its own drawing. Servo leaves
//! the list of a `<select>` to the host: the engine keeps Servo's request on
//! its thread, and hands the window a bounded copy of the list to draw.
//!
//! Servo's requests answer themselves when dropped, with no change, so a
//! request this module lets go of is always answered: when another one
//! replaces it, when the page navigates or hides, and when its tab closes.
//! The window never waits for the engine and the engine never waits for the
//! window: the list waits in the page's [`super::Mailbox`], and the choice
//! comes back as a [`super::Command::Chosen`].
use super::Notifier;
use servo::{
    EmbedderControl, EmbedderControlId, SelectElement, SelectElementOption,
    SelectElementOptionOrOptgroup,
};
use std::{
    cell::{Cell, RefCell},
    sync::{Arc, Mutex, PoisonError},
};

/// Rows of one list that reach the window: options and group labels. A page
/// with more shows its first ones.
const ROWS: usize = 2000;

/// Characters of one label that reach the window.
const LABEL: usize = 200;

/// One line of a list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::browser) enum Row {
    /// The label of an `<optgroup>`, which nobody chooses.
    Group(String),
    Choice {
        /// Servo's name for the option: its place among the page's options.
        id: usize,
        label: String,
        disabled: bool,
        /// Whether it belongs to the group labelled above it.
        grouped: bool,
    },
}

/// The list of one open `<select>`, as the window draws it. Page text:
/// bounded here, and only ever shown.
#[derive(Debug, Eq, PartialEq)]
pub(in crate::browser) struct Menu {
    /// Names this opening of a list for its page; later ones are greater.
    pub(in crate::browser) number: u64,
    /// The control's left, top, width, and height as Servo reports them:
    /// CSS pixels of the control's own document.
    pub(in crate::browser) control: [i32; 4],
    pub(in crate::browser) rows: Vec<Row>,
    /// The options chosen as the list opened.
    pub(in crate::browser) chosen: Vec<usize>,
    pub(in crate::browser) multiple: bool,
}

/// The list a page has open, for its window to read. Only the latest
/// counts, so nothing queues however often a page opens one.
#[derive(Default)]
pub(in crate::browser) struct Listed(Mutex<Option<Arc<Menu>>>);

impl Listed {
    fn put(&self, menu: Option<Arc<Menu>>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = menu;
    }

    pub(in crate::browser) fn get(&self) -> Option<Arc<Menu>> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Whether `c` changes the direction text is laid out in. A label with one
/// could show its row in another order than the page sent it.
fn directs(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

/// `label` as one bounded line, in the order of its letters: a character
/// that ends a line becomes a space, and one that directs text is left out.
fn label(label: &str) -> String {
    let mut shown = label.chars().filter(|c| !directs(*c)).map(|c| {
        if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') {
            ' '
        } else {
            c
        }
    });
    let mut line: String = shown.by_ref().take(LABEL).collect();
    if shown.next().is_some() {
        line.push('…');
    }
    line
}

fn choice(option: &SelectElementOption, grouped: bool) -> Row {
    Row::Choice {
        id: option.id,
        label: label(&option.label),
        disabled: option.is_disabled,
        grouped,
    }
}

/// The first [`ROWS`] lines of `options`.
fn rows(options: &[SelectElementOptionOrOptgroup]) -> Vec<Row> {
    let lines = options.iter().flat_map(|entry| {
        let (group, options, grouped) = match entry {
            SelectElementOptionOrOptgroup::Option(option) => {
                (None, std::slice::from_ref(option), false)
            }
            // Servo also groups options that stand alone, without a label.
            SelectElementOptionOrOptgroup::Optgroup {
                label: name,
                options,
            } => {
                let name = Some(label(name)).filter(|name| !name.trim().is_empty());
                let grouped = name.is_some();
                (name.map(Row::Group), options.as_slice(), grouped)
            }
        };
        group
            .into_iter()
            .chain(options.iter().map(move |option| choice(option, grouped)))
    });
    lines.take(ROWS).collect()
}

/// Servo's name for the last option in `rows`. Servo names a page's
/// options by their place, from 0 up, and `rows` holds the first ones: an
/// option is in the list when its name is not greater.
fn last(rows: &[Row]) -> Option<usize> {
    rows.iter().rev().find_map(|row| match row {
        Row::Choice { id, .. } => Some(*id),
        Row::Group(_) => None,
    })
}

/// The options of `selected` that a list ending at option `last` shows as
/// chosen: one, or each of them for a list of several.
fn chosen(selected: &[usize], last: usize, multiple: bool) -> Vec<usize> {
    let shown = selected.iter().copied().filter(|id| *id <= last);
    shown.take(if multiple { ROWS } else { 1 }).collect()
}

/// What Servo is answered for `choice`, made in a list that ends at option
/// `last`. Servo unselects every option the answer leaves out, so the
/// options of `selected` that a list of several did not show stay in it.
fn answered(mut choice: Vec<usize>, selected: &[usize], last: usize, multiple: bool) -> Vec<usize> {
    choice.retain(|id| *id <= last);
    if multiple {
        choice.extend(selected.iter().copied().filter(|id| *id > last));
    }
    choice
}

/// The one request a page has open, and the number of its list. A request
/// answers its page when it is dropped, so the slot only ever lets go of
/// one by dropping it or by handing it to the caller: never twice, and
/// never without an answer.
struct Slot<T> {
    open: RefCell<Option<(u64, T)>>,
    last: Cell<u64>,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self {
            open: RefCell::new(None),
            last: Cell::new(0),
        }
    }
}

impl<T> Slot<T> {
    /// Holds `request` in place of the open one, which is dropped. The
    /// number of its list, greater than every earlier one.
    fn hold(&self, request: T) -> u64 {
        let number = self.last.get() + 1;
        self.last.set(number);
        let old = self.open.replace(Some((number, request)));
        drop(old);
        number
    }

    /// Drops the open request, if `named` says it is the one meant. Whether
    /// one was dropped.
    fn close(&self, named: impl FnOnce(&T) -> bool) -> bool {
        let old = {
            let mut open = self.open.borrow_mut();
            match &*open {
                Some((_, request)) if named(request) => open.take(),
                _ => None,
            }
        };
        old.is_some()
    }

    /// The open request, if its list is `number`. Another list's request
    /// stays open.
    fn take(&self, number: u64) -> Option<T> {
        let mut open = self.open.borrow_mut();
        match &*open {
            Some((held, _)) if *held == number => open.take().map(|(_, request)| request),
            _ => None,
        }
    }
}

/// The request of one page's open `<select>`, on the engine's thread, with
/// the name of the last option its list shows.
pub(super) struct Held {
    open: Slot<(SelectElement, usize)>,
    listed: Arc<super::Mailbox>,
    notifier: Arc<Notifier>,
}

impl Held {
    pub(super) fn new(listed: Arc<super::Mailbox>, notifier: Arc<Notifier>) -> Self {
        Self {
            open: Slot::default(),
            listed,
            notifier,
        }
    }

    /// Takes what the page asks its host to show. A single or multiple
    /// `<select>` gets its list; every other control is dismissed, which
    /// answers the page as if a person had closed it, and named for the log.
    pub(super) fn show(&self, control: EmbedderControl) -> Option<&'static str> {
        // i18n: data
        let dismissed = match control {
            EmbedderControl::SelectElement(select) => {
                self.list(select);
                return None;
            }
            // Shows nothing and waits for no answer.
            EmbedderControl::InputMethod(_) => return None,
            // Dropped: Servo keeps the color the control has.
            EmbedderControl::ColorPicker(_) => "color",
            // A page reads no file of this computer through a picker.
            EmbedderControl::FilePicker(picker) => {
                picker.dismiss();
                "file"
            }
            // `alert` returns, `confirm` answers false, `prompt` null.
            EmbedderControl::SimpleDialog(dialog) => {
                dialog.dismiss();
                "dialog"
            }
            EmbedderControl::ContextMenu(menu) => {
                menu.dismiss();
                "menu"
            }
        };
        Some(dismissed)
    }

    fn list(&self, select: SelectElement) {
        let rows = rows(select.options());
        // The request it replaces, and one with nothing to choose, are
        // dropped: Servo is answered that nothing changed.
        self.open.close(|_| true);
        let Some(last) = last(&rows) else {
            self.tell(None);
            return;
        };
        let multiple = select.allow_select_multiple();
        let chosen = chosen(&select.selected_options(), last, multiple);
        let place = select.position();
        let control = [place.min.x, place.min.y, place.width(), place.height()];
        let menu = Menu {
            number: self.open.hold((select, last)),
            control,
            rows,
            chosen,
            multiple,
        };
        self.tell(Some(Arc::new(menu)));
    }

    fn tell(&self, menu: Option<Arc<Menu>>) {
        self.listed.list.put(menu);
        self.notifier.notify();
    }

    /// Servo no longer wants the control `id`: its element left the page.
    pub(super) fn hide(&self, id: EmbedderControlId) {
        if self.open.close(|(select, _)| select.id() == id) {
            self.tell(None);
        }
    }

    /// Closes the list without a choice: the page navigates, hides, or
    /// loses the keyboard.
    pub(super) fn dismiss(&self) {
        if self.open.close(|_| true) {
            self.tell(None);
        }
    }

    /// The window's answer to list `number`: the options chosen, or none
    /// when the list was closed. An answer to a list that is gone is late,
    /// and Servo was answered when it went.
    pub(super) fn answer(&self, number: u64, choice: Option<Vec<usize>>) {
        let Some((mut select, last)) = self.open.take(number) else {
            return;
        };
        if let Some(choice) = choice {
            let multiple = select.allow_select_multiple();
            let kept = answered(choice, &select.selected_options(), last, multiple);
            select.select(kept);
        }
        select.submit();
        self.tell(None);
    }
}

#[cfg(test)]
mod tests;
