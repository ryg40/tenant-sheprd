//! The pointer's shape over a page. Servo tells a page's delegate, on the
//! engine's thread, which shape the page asks for; the engine keeps the
//! latest one in the page's [`super::Mailbox`] and wakes its window only
//! when what the window shows changes. The window reads it as it paints the
//! page. Only the latest shape counts, so nothing queues however often a
//! page changes it.
//!
//! Servo reports a shape only when it differs from the one it keeps, a
//! single one for all documents of the page. So after a reset, which shows
//! the arrow while Servo keeps its old shape, the engine reads Servo's
//! shape once Servo has handled a pointer move sent after the reset.
//!
//! A new document has no shape and no pointer place of its own yet, so the
//! first move that Servo tests in it sets Servo's shape before Servo
//! reports the move as handled. The engine reads that shape when the move
//! is one it still keeps, also when the shape equals Servo's old one and
//! Servo reported nothing; a move that later ones have replaced reads
//! nothing, and a later move then does.
//!
//! Not guaranteed for a document that the pinned Servo restores from its
//! history: it keeps its own last shape and sends none that equals it,
//! while Servo keeps the shape of the document shown before. A handled move
//! can then read that other document's shape, and the window shows it until
//! the shape under the pointer changes. The host cannot see this case. A
//! move that Servo handled without a test of the pointer's place, because
//! its document went away or the document's test found nothing, also reads
//! Servo's last shape.
//!
//! A page is untrusted: no shape it asks for hides the pointer.
use super::{Mailbox, Notifier, Open};
use gpui::CursorStyle;
use servo::{Cursor, InputEvent, InputEventId, InputEventResult, WebView};
use std::{
    cell::RefCell,
    sync::{Arc, Mutex, PoisonError},
};

/// The moves sent since a reset that the engine keeps besides the first.
/// Each stays for as many later moves, so its handling has time to arrive
/// while the pointer keeps moving.
const PROBES: usize = 16;

/// The shape the window shows over one page: the arrow until the page
/// asks for another.
#[derive(Default)]
pub(in crate::browser) struct Shape(Mutex<CursorStyle>);

impl Shape {
    /// Keeps `style`. Whether it differs from the shape kept before.
    fn put(&self, style: CursorStyle) -> bool {
        let mut kept = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        std::mem::replace(&mut *kept, style) != style
    }

    /// The shape to show while the pointer is over the page.
    pub(in crate::browser) fn get(&self) -> CursorStyle {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The moves sent since a reset whose handling reads Servo's shape: the
/// first, which counts however far Servo lags behind, and the latest
/// [`PROBES`], so a move Servo never handles keeps no window waiting.
#[derive(Default)]
struct Probes {
    first: Option<InputEventId>,
    /// The oldest first.
    latest: [Option<InputEventId>; PROBES],
}

impl Probes {
    fn add(&mut self, id: InputEventId) {
        if self.first.is_none() {
            self.first = Some(id);
            return;
        }
        // The oldest one makes room.
        self.latest.rotate_left(1);
        if let Some(newest) = self.latest.last_mut() {
            *newest = Some(id);
        }
    }

    /// Takes `id` out. Whether it was there.
    fn take(&mut self, id: InputEventId) -> bool {
        let mut found = false;
        for kept in std::iter::once(&mut self.first).chain(&mut self.latest) {
            if *kept == Some(id) {
                *kept = None;
                found = true;
            }
        }
        found
    }
}

/// The engine's hold on one page's shape. When the engine lets go of the
/// page, as the page closes or the engine ends, its window shows the arrow
/// again: no shape of a page that is gone stays behind.
pub(super) struct Pointer {
    mailbox: Arc<Mailbox>,
    notifier: Arc<Notifier>,
    /// The moves kept since the last reset, which shows the arrow while
    /// Servo keeps the shape it had before. `None` while the window shows
    /// Servo's own shape: Servo reports each change of it.
    reset: RefCell<Option<Probes>>,
}

impl Pointer {
    pub(super) fn new(open: &Open) -> Self {
        Self::with(open.mailbox.clone(), open.notifier.clone())
    }

    fn with(mailbox: Arc<Mailbox>, notifier: Arc<Notifier>) -> Self {
        Self {
            mailbox,
            notifier,
            // Servo's shape for a new page is the hand, not the arrow.
            reset: RefCell::new(Some(Probes::default())),
        }
    }

    /// Shows `cursor`, the shape Servo reports for the page.
    pub(super) fn show(&self, cursor: Cursor) {
        *self.reset.borrow_mut() = None;
        self.put(cursor);
    }

    /// Shows the arrow until Servo has tested a pointer place in what the
    /// page shows now: a new or restored document, or a page shown again.
    pub(super) fn reset(&self) {
        *self.reset.borrow_mut() = Some(Probes::default());
        self.put(Cursor::Default);
    }

    /// Hands `event` to `view`, the page's view. Every pointer move the
    /// engine sends goes through here, so its handling can read Servo's
    /// shape. Returns the id Servo gave the event.
    pub(super) fn send(&self, view: &WebView, event: InputEvent) -> InputEventId {
        let moved = matches!(event, InputEvent::MouseMove(_));
        let id = view.notify_input_event(event);
        if moved {
            self.sent(id);
        }
        id
    }

    /// Notes a pointer move the engine sent the page.
    fn sent(&self, id: InputEventId) {
        if let Some(probes) = &mut *self.reset.borrow_mut() {
            probes.add(id);
        }
    }

    /// Servo has handled the input `id`. When it is a move sent since the
    /// last reset, `shape` reads the shape Servo keeps, which is then the
    /// one for the pointer's place in what the page shows now.
    pub(super) fn handled(
        &self,
        id: InputEventId,
        result: InputEventResult,
        shape: impl FnOnce() -> Cursor,
    ) {
        let probe = self
            .reset
            .borrow_mut()
            .as_mut()
            .is_some_and(|probes| probes.take(id));
        // Servo tested no place for a move it failed to dispatch: a later
        // move must.
        if probe && !result.contains(InputEventResult::DispatchFailed) {
            self.show(shape());
        }
    }

    /// Keeps the shape for `cursor`, and wakes the window when that changes
    /// what it shows.
    fn put(&self, cursor: Cursor) {
        if self.mailbox.cursor.put(style(cursor)) {
            self.notifier.notify();
        }
    }
}

impl Drop for Pointer {
    fn drop(&mut self) {
        self.put(Cursor::Default);
    }
}

/// The shape GPUI shows for `cursor`: the same one, the nearest one, or the
/// arrow. `cursor: none` shows the arrow, so a page cannot hide the
/// pointer.
fn style(cursor: Cursor) -> CursorStyle {
    match cursor {
        Cursor::Pointer => CursorStyle::PointingHand,
        Cursor::Text => CursorStyle::IBeam,
        Cursor::VerticalText => CursorStyle::IBeamCursorForVerticalLayout,
        Cursor::Crosshair | Cursor::Cell => CursorStyle::Crosshair,
        Cursor::Grab => CursorStyle::OpenHand,
        Cursor::Grabbing => CursorStyle::ClosedHand,
        Cursor::NotAllowed | Cursor::NoDrop => CursorStyle::OperationNotAllowed,
        Cursor::Alias => CursorStyle::DragLink,
        Cursor::Copy => CursorStyle::DragCopy,
        Cursor::ContextMenu => CursorStyle::ContextualMenu,
        Cursor::EResize => CursorStyle::ResizeRight,
        Cursor::WResize => CursorStyle::ResizeLeft,
        Cursor::EwResize => CursorStyle::ResizeLeftRight,
        Cursor::NResize => CursorStyle::ResizeUp,
        Cursor::SResize => CursorStyle::ResizeDown,
        Cursor::NsResize => CursorStyle::ResizeUpDown,
        // GPUI's platforms draw `ResizeUpLeftDownRight` as `nwse-resize`
        // and `ResizeUpRightDownLeft` as `nesw-resize`, whatever their
        // comments say. GPUI has no arrow for one corner.
        Cursor::NwseResize | Cursor::NwResize | Cursor::SeResize => {
            CursorStyle::ResizeUpLeftDownRight
        }
        Cursor::NeswResize | Cursor::NeResize | Cursor::SwResize => {
            CursorStyle::ResizeUpRightDownLeft
        }
        Cursor::ColResize => CursorStyle::ResizeColumn,
        Cursor::RowResize => CursorStyle::ResizeRow,
        // GPUI has no shape for the others.
        Cursor::None
        | Cursor::Default
        | Cursor::Help
        | Cursor::Progress
        | Cursor::Wait
        | Cursor::Move
        | Cursor::AllScroll
        | Cursor::ZoomIn
        | Cursor::ZoomOut => CursorStyle::Arrow,
    }
}

#[cfg(test)]
mod tests;
