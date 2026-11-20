//! A page's clipboard. Servo asks its host to read, write, or clear the
//! system clipboard on the engine's thread ([`ClipboardDelegate`]); the
//! app's clipboard lives on the UI thread. A request crosses over as a
//! `<select>` list does ([`super::controls`]): the engine keeps Servo's
//! handle to a read and names it by a number, the window finds what the
//! page asked in its [`super::Mailbox`] when the window's notifier wakes it,
//! and the answer to a read comes back as a [`super::Command::Pasted`].
//! Neither thread waits for the other. Text only: no files, images, or HTML.
//!
//! The pinned Servo, as 0.8.0 did, reads the clipboard for the paste key
//! (or an `InputEvent::EditingAction`), and fires the page's paste event
//! with the text; and for `navigator.clipboard.readText()`, which exists
//! only with `dom_async_clipboard_enabled` and then reads without asking
//! anyone. It writes for the copy and cut keys, for a copy or cut event the
//! page fills itself (which clears the clipboard first), for
//! `navigator.clipboard.writeText()` behind the same preference, and for
//! its context menu, which the host closes. `document.execCommand` runs no
//! clipboard command. So the host holds the rule itself, whatever Servo's
//! preferences say, for a page that is shown and has the keyboard:
//!
//! - The window's paste key grants the page one read, and its copy or cut
//!   key grants writes, each for [`GESTURE`] ([`Grant`]). The window puts
//!   the grant on the key as it sends it ([`Pressed`]): a key that waited
//!   past a revocation or its deadline still reaches the page, with no
//!   grant. Every other read is refused, and every other write changes
//!   nothing.
//! - A grant ends at its deadline. Every grant of a page ends when the page
//!   hides, loses the keyboard, starts a load, a reload, or a history step,
//!   shows another address, or closes, and when its window stops being the
//!   active one: each of them advances the page's generation, which a grant
//!   must still have. The grant of a new window ([`super::popup`]) has the
//!   same generation, and ends with them. The engine checks a grant as the
//!   key arrives, as the page asks, as the window's answer arrives, and as
//!   it hands a write over; the window checks it before it touches the
//!   clipboard.
//! - One copy or cut is one change of the clipboard. The text of a
//!   selection goes to the window as Servo makes it, ahead of every later
//!   read of the page, which the window serves after the write. A copy
//!   event the page fills waits ([`Filled`]): Servo sends a clear first,
//!   then each text of the event, and no sign of the last one. The copy
//!   ends, and goes to the window, at the next clear, which starts
//!   another copy, at the page's next read, or at Servo's report that it
//!   handled a copy or cut key, whichever is first. Its last text is the
//!   change, a clear when it has none; with one text over [`TEXT`] the
//!   whole copy changes nothing. A copy that waits past the end of its own
//!   grant changes nothing either, and is dropped before a later key's
//!   copy, with the keys of that grant ([`Clipboard::retire`]).
//!
//! Servo names only the `WebView` of a request, not its frame or document:
//! with `dom_async_clipboard_enabled`, which must stay off, a script of any
//! frame could take the read that a paste key grants.
//!
//! Servo's paste waits on the page's script thread for its answer, so no
//! read is left open: a full waiting place, the end of the page's grants,
//! and its deadline drop it, and a dropped request answers Servo a failure
//! by itself. The engine's loop wakes at the deadline of the first read that
//! waits ([`Clipboard::deadline`]), whatever the window does.
use super::{Command, Mailbox, Notifier, PageKey};
use servo::{
    ClipboardAction, ClipboardDelegate, InputEvent, InputEventId, Key, KeyState, Modifiers,
    StringRequest, WebView,
};
use std::{
    cell::{Cell, RefCell},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Bytes of text one copy or one paste carries at most. A longer copy
/// changes nothing, and a paste of longer text gets none.
pub(in crate::browser) const TEXT: usize = 1 << 20;

/// Reads of one page that wait for its window at once; one more is refused.
const WAITING: usize = 4;

/// How long after the paste, copy, or cut key the page may use it: the
/// transient activation that browsers give a page after a key. Servo asks
/// while it handles the key, well within it, also in a debug build.
const GESTURE: Duration = Duration::from_secs(5);

/// A write a page asked for.
#[derive(Debug, Eq, PartialEq)]
pub(in crate::browser) enum Write {
    // keep-sorted start
    Clear,
    Text(String),
    // keep-sorted end
}

/// What lets a page use the clipboard: the key the window sent it, until
/// its deadline, while the page's grants keep the generation it was given
/// in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::browser) struct Grant {
    deadline: Instant,
    generation: u64,
}

/// What a page asked of the clipboard and its window has not done yet,
/// each with the grant that let it.
#[derive(Debug, Default)]
pub(in crate::browser) struct Asks {
    /// Only the latest write counts.
    pub(in crate::browser) write: Option<(Write, Grant)>,
    /// The reads, by number, oldest first.
    pub(in crate::browser) reads: Vec<(u64, Grant)>,
}

/// A page's [`Asks`], shared with its window, and the generation of the
/// page's grants, which either side advances to end them all.
#[derive(Default)]
pub(in crate::browser) struct Waiting {
    asks: Mutex<Asks>,
    generation: AtomicU64,
}

impl Waiting {
    fn with<T>(&self, change: impl FnOnce(&mut Asks) -> T) -> T {
        change(&mut self.asks.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Everything that waits, for the window to do now.
    pub(in crate::browser) fn take(&self) -> Asks {
        self.with(std::mem::take)
    }

    /// Whether `grant` still lets the page use the clipboard `now`.
    pub(in crate::browser) fn holds(&self, grant: Grant, now: Instant) -> bool {
        now < grant.deadline && grant.generation == self.generation.load(Ordering::Acquire)
    }

    /// Ends every grant of the page. Returns what waited for the window,
    /// which no longer holds.
    pub(in crate::browser) fn revoke(&self) -> Asks {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.take()
    }

    /// The grant the window puts on `event` as it sends it `now`, if it is
    /// a paste, a copy, or a cut.
    pub(in crate::browser) fn pressed(&self, event: &InputEvent, now: Instant) -> Option<Pressed> {
        Some(Pressed {
            gesture: gesture(event)?,
            grant: self.grant(now, GESTURE),
        })
    }

    /// A grant of the page for `length` from `now`, which ends with every
    /// other grant of the page: also the new window that a press lets the
    /// page open ([`super::popup`]).
    pub(in crate::browser) fn grant(&self, now: Instant, length: Duration) -> Grant {
        Grant {
            // At the end of the clock, a grant that has ended.
            deadline: now.checked_add(length).unwrap_or(now),
            generation: self.generation.load(Ordering::Acquire),
        }
    }
}

/// A paste, copy, or cut key, with the grant it had as the window sent it.
#[derive(Clone, Copy, Debug)]
pub(in crate::browser) struct Pressed {
    gesture: Gesture,
    grant: Grant,
}

/// What a key or an editing action asks of the clipboard.
#[derive(Clone, Copy, Debug)]
enum Gesture {
    // keep-sorted start
    /// A copy or a cut.
    Copy,
    Paste,
    // keep-sorted end
}

/// The keys Servo turns into a paste, a copy, or a cut
/// (`editing_action_from_keyboard_event`): V, C, or X going down with Ctrl
/// and nothing else held; and the editing actions.
fn gesture(event: &InputEvent) -> Option<Gesture> {
    let key = match event {
        InputEvent::EditingAction(ClipboardAction::Paste) => return Some(Gesture::Paste),
        InputEvent::EditingAction(ClipboardAction::Copy | ClipboardAction::Cut) => {
            return Some(Gesture::Copy);
        }
        InputEvent::Keyboard(key) => &key.event,
        _ => return None,
    };
    if key.state != KeyState::Down || key.modifiers != Modifiers::CONTROL {
        return None;
    }
    let Key::Character(typed) = &key.key else {
        return None;
    };
    if typed.eq_ignore_ascii_case("v") {
        Some(Gesture::Paste)
    } else if typed.eq_ignore_ascii_case("c") || typed.eq_ignore_ascii_case("x") {
        Some(Gesture::Copy)
    } else {
        None
    }
}

/// Ends the grants of the page whose document `command` replaces: a load, a
/// reload, or a history step. The pinned Servo reports no start of the load
/// for a reload it is asked for (`WebView::reload` sets the status itself),
/// so the host calls this as it takes each command, before Servo hears of
/// it. `page` finds the clipboard of a page by its key.
pub(super) fn before_command<'a, R: 'a>(
    command: &Command,
    page: impl FnOnce(PageKey) -> Option<&'a Clipboard<R>>,
) {
    let (Command::Load(key, _) | Command::Reload(key) | Command::History(key, _)) = command else {
        return;
    };
    if let Some(clipboard) = page(*key) {
        clipboard.forget();
    }
}

/// Copy and cut keys that Servo has not reported handled, which the engine
/// remembers at most: past them it forgets the oldest, and only whether
/// more than one waits matters.
const UNHANDLED: usize = 8;

/// The copy and cut keys the window sent the page.
struct Copying {
    /// The grant of the latest key.
    grant: Grant,
    /// The keys as Servo named them, each with its own grant, oldest
    /// first, until Servo reports them handled or their grant ends: a clear
    /// until then starts a copy that waits for its texts.
    keys: Vec<(InputEventId, Grant)>,
}

/// A copy or cut event the page filled, from Servo's clear until the copy
/// ends: nothing of it has reached the window.
struct Filled {
    grant: Grant,
    /// The last text of the copy, which the clipboard gets.
    text: Option<String>,
    /// Whether Servo sent a text of this copy.
    texts: bool,
    /// Whether one text was over [`TEXT`]: the copy then changes nothing.
    over: bool,
}

/// A read that waits for the window.
struct Held<R> {
    number: u64,
    grant: Grant,
    request: R,
}

/// One page's clipboard on the engine's thread: Servo's reads (`R`) that
/// wait for the window, and the keys that let the page use the clipboard.
pub(super) struct Clipboard<R = StringRequest> {
    reads: RefCell<Vec<Held<R>>>,
    next: Cell<u64>,
    shown: Cell<bool>,
    focused: Cell<bool>,
    /// The paste key's grant, until a read uses it.
    paste: Cell<Option<Grant>>,
    copy: RefCell<Option<Copying>>,
    /// The copy the page fills now, if Servo sent its clear and not its end.
    filled: RefCell<Option<Filled>>,
    mailbox: Arc<Mailbox>,
    notifier: Arc<Notifier>,
}

impl<R> Clipboard<R> {
    pub(super) fn new(mailbox: Arc<Mailbox>, notifier: Arc<Notifier>) -> Self {
        Self {
            reads: RefCell::default(),
            next: Cell::new(0),
            shown: Cell::new(false),
            focused: Cell::new(false),
            paste: Cell::new(None),
            copy: RefCell::new(None),
            filled: RefCell::new(None),
            mailbox,
            notifier,
        }
    }

    /// Notes a key the window sent the page, which Servo named `id`, as
    /// the engine hands it to Servo `now`. A key that waited past a
    /// revocation or past its deadline gets no grant.
    pub(super) fn input(&self, pressed: Option<Pressed>, id: InputEventId, now: Instant) {
        let Some(Pressed { gesture, grant }) = pressed else {
            return;
        };
        let grant = Some(grant).filter(|grant| self.mailbox.clipboard.holds(*grant, now));
        match gesture {
            Gesture::Paste => self.paste.set(grant),
            Gesture::Copy => {
                let Some(grant) = grant else {
                    self.copy.borrow_mut().take();
                    return;
                };
                self.retire(now);
                let mut copy = self.copy.borrow_mut();
                let copying = copy.get_or_insert_with(|| Copying {
                    grant,
                    keys: Vec::new(),
                });
                copying.grant = grant;
                if copying.keys.len() == UNHANDLED {
                    copying.keys.remove(0);
                }
                copying.keys.push((id, grant));
            }
        }
    }

    /// Servo has handled the copy or cut key it named `id`, and every one
    /// before it: the copy the page filled has all its texts, and goes to
    /// the window now. Servo sends this report between two events, never
    /// inside one copy.
    pub(super) fn handled(&self, id: InputEventId, now: Instant) {
        {
            let mut copy = self.copy.borrow_mut();
            let Some(copying) = copy.as_mut() else {
                return;
            };
            let Some(place) = copying.keys.iter().position(|(key, _)| *key == id) else {
                return;
            };
            copying.keys.drain(..=place);
        }
        self.end_copy(now);
    }

    pub(super) fn shown(&self, shown: bool) {
        self.shown.set(shown);
        if !shown {
            self.forget();
        }
    }

    pub(super) fn focused(&self, focused: bool) {
        self.focused.set(focused);
        if !focused {
            self.forget();
        }
    }

    /// Ends every grant of the page: the keys pressed before, what waits
    /// for the window, and the reads that wait, which answers Servo a
    /// failure. They were for the document the page showed with the
    /// keyboard.
    pub(super) fn forget(&self) {
        self.paste.set(None);
        self.copy.borrow_mut().take();
        self.filled.borrow_mut().take();
        self.mailbox.clipboard.revoke();
        let dropped = std::mem::take(&mut *self.reads.borrow_mut());
        drop(dropped);
    }

    /// Drops the copy the page fills and each unhandled key whose own grant
    /// no longer holds `now`, before a later copy: a copy under a grant
    /// that ended changes nothing, and takes no text of another copy with
    /// it. Servo's late report for such a key then ends no copy.
    fn retire(&self, now: Instant) {
        let waiting = &self.mailbox.clipboard;
        self.filled
            .borrow_mut()
            .take_if(|filled| !waiting.holds(filled.grant, now));
        if let Some(copying) = self.copy.borrow_mut().as_mut() {
            copying.keys.retain(|(_, grant)| waiting.holds(*grant, now));
        }
    }

    fn keyboard(&self) -> bool {
        self.shown.get() && self.focused.get()
    }

    /// When the first read that waits must be answered, if one waits: the
    /// engine wakes then to answer it a failure, if its window did not.
    pub(super) fn deadline(&self) -> Option<Instant> {
        self.reads
            .borrow()
            .iter()
            .map(|held| held.grant.deadline)
            .min()
    }

    /// Drops each read whose grant no longer holds `now`, which answers
    /// Servo a failure, and its number from what the window finds.
    pub(super) fn expire(&self, now: Instant) {
        let waiting = &self.mailbox.clipboard;
        let ended: Vec<Held<R>> = self
            .reads
            .borrow_mut()
            .extract_if(.., |held| !waiting.holds(held.grant, now))
            .collect();
        if ended.is_empty() {
            return;
        }
        waiting.with(|asks| {
            asks.reads
                .retain(|(number, _)| ended.iter().all(|held| held.number != *number));
        });
        drop(ended);
    }

    /// Hands Servo's read to the window, or drops it.
    fn read(&self, request: R, now: Instant) {
        // Servo made the copy before this read, which is of another event.
        self.end_copy(now);
        // One press lets one read through, answered or not.
        let grant = self.paste.take().filter(|grant| {
            self.mailbox.clipboard.holds(*grant, now)
                && self.keyboard()
                && self.reads.borrow().len() < WAITING
        });
        let Some(grant) = grant else {
            drop(request);
            return;
        };
        let number = self.next.replace(self.next.get().wrapping_add(1));
        self.reads.borrow_mut().push(Held {
            number,
            grant,
            request,
        });
        self.mailbox
            .clipboard
            .with(|asks| asks.reads.push((number, grant)));
        self.notifier.notify();
    }

    /// Takes Servo's write: the text of a selection goes to the window, and
    /// a copy the page fills waits for its end.
    fn write(&self, write: Write, now: Instant) {
        let Some(grant) = self.copy.borrow().as_ref().map(|copying| copying.grant) else {
            return;
        };
        if !self.keyboard() || !self.mailbox.clipboard.holds(grant, now) {
            return;
        }
        // This write is of a grant that holds: what an ended one left is
        // no part of it.
        self.retire(now);
        let unhandled = self
            .copy
            .borrow()
            .as_ref()
            .map_or(0, |copying| copying.keys.len());
        let text = match write {
            Write::Clear => {
                // One copy has one clear: this one starts another.
                self.end_copy(now);
                if unhandled == 0 {
                    // No key whose report would end the copy.
                    self.hand_over(Write::Clear, grant, now);
                } else {
                    *self.filled.borrow_mut() = Some(Filled {
                        grant,
                        text: None,
                        texts: false,
                        over: false,
                    });
                }
                return;
            }
            Write::Text(text) => text,
        };
        // After a text of a filled copy, a text is the copy's next one, or
        // the selection that the next key copied: with one key unhandled,
        // it is the copy's.
        let next = self
            .filled
            .borrow()
            .as_ref()
            .is_some_and(|filled| filled.texts && unhandled > 1);
        if next {
            self.end_copy(now);
        }
        if let Some(filled) = self.filled.borrow_mut().as_mut() {
            filled.texts = true;
            filled.over |= text.len() > TEXT;
            // A copy over the limit keeps no text.
            filled.text = (!filled.over).then_some(text);
            return;
        }
        if text.len() <= TEXT {
            self.hand_over(Write::Text(text), grant, now);
        }
    }

    /// Ends the copy the page fills, if one waits: hands its last text to
    /// the window, or its clear when it has no text, or nothing when a text
    /// of it was over [`TEXT`].
    fn end_copy(&self, now: Instant) {
        let Some(filled) = self.filled.borrow_mut().take() else {
            return;
        };
        if filled.over {
            return;
        }
        let write = filled.text.map_or(Write::Clear, Write::Text);
        self.hand_over(write, filled.grant, now);
    }

    /// Hands `write` to the window, if its grant still holds. Only the
    /// latest write waits there.
    fn hand_over(&self, write: Write, grant: Grant, now: Instant) {
        if !self.keyboard() || !self.mailbox.clipboard.holds(grant, now) {
            return;
        }
        self.mailbox
            .clipboard
            .with(|asks| asks.write = Some((write, grant)));
        self.notifier.notify();
    }

    /// The window's answer to read `number`, arrived `now`: the clipboard's
    /// text, which `succeed` hands Servo, or none. An answer after the
    /// read's deadline, or after its grant ended, is a failure. A read that
    /// is gone was dropped, which answered Servo.
    fn settle(
        &self,
        number: u64,
        text: Option<String>,
        now: Instant,
        succeed: impl FnOnce(R, String),
    ) {
        let held = {
            let mut reads = self.reads.borrow_mut();
            let Some(place) = reads.iter().position(|held| held.number == number) else {
                return;
            };
            reads.remove(place)
        };
        let holds = self.keyboard() && self.mailbox.clipboard.holds(held.grant, now);
        match text.filter(|_| holds) {
            Some(text) => succeed(held.request, text),
            // Dropped, it answers Servo a failure.
            None => drop(held.request),
        }
    }
}

impl Clipboard {
    /// The window's answer to read `number`: the clipboard's text, or none.
    pub(super) fn answer(&self, number: u64, text: Option<String>) {
        self.settle(number, text, Instant::now(), StringRequest::success);
    }
}

impl ClipboardDelegate for Clipboard {
    fn clear(&self, _: WebView) {
        self.write(Write::Clear, Instant::now());
    }

    fn get_text(&self, _: WebView, request: StringRequest) {
        self.read(request, Instant::now());
    }

    fn set_text(&self, _: WebView, text: String) {
        self.write(Write::Text(text), Instant::now());
    }
}

#[cfg(test)]
mod tests;
