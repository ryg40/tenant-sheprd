//! What a page asked of the clipboard, done on the UI thread, where the
//! app's clipboard is: the engine decided that the page may
//! ([`crate::browser::engine::clipboard`]), and the window does it only
//! while it is the active window, the page still has the keyboard, and the
//! grant of the ask still holds. Text only: a copied file or image pastes
//! nothing.
use super::Page;
use crate::browser::engine::{
    Command,
    clipboard::{Grant, TEXT, Write},
};
use gpui::{App, ClipboardEntry, ClipboardItem};
use std::time::Instant;

impl Page {
    /// Writes what the page copied and answers its reads, in a window that
    /// is `active`. Cheap when it asked nothing.
    pub(in crate::browser) fn serve_clipboard(&self, active: bool, cx: &App) {
        let asks = self.mailbox.clipboard.take();
        let now = Instant::now();
        let serves = active && self.shown.get() && self.focused.get();
        let holds = |grant: Grant| serves && self.mailbox.clipboard.holds(grant, now);
        // The write before the reads: Servo made it before each read that
        // waits with it, so a paste after a cut gets the cut's text.
        if let Some((write, grant)) = asks.write
            && holds(grant)
        {
            let text = match write {
                Write::Text(text) => text,
                // GPUI empties no clipboard; empty text is the nearest.
                Write::Clear => String::new(),
            };
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        let (served, refused): (Vec<_>, Vec<_>) =
            asks.reads.into_iter().partition(|(_, grant)| holds(*grant));
        for (number, _) in refused {
            self.answer_paste(number, None);
        }
        if served.is_empty() {
            return;
        }
        // Read once for every read that waits, and never again for them.
        let text = cx.read_from_clipboard().and_then(|item| plain_text(&item));
        for (number, _) in served {
            self.answer_paste(number, text.clone());
        }
    }

    /// The window stopped being the active one: every grant of the page
    /// ends, and the reads that waited are refused.
    pub(in crate::browser) fn end_clipboard_grants(&self) {
        for (number, _) in self.mailbox.clipboard.revoke().reads {
            self.answer_paste(number, None);
        }
    }

    /// Hands the engine the answer to read `number`. An answer that its
    /// queue refuses is dropped: the engine fails the read at its deadline
    /// by itself.
    fn answer_paste(&self, number: u64, text: Option<String>) {
        self.send(|key| Command::Pasted(key, number, text));
    }
}

/// The text of `item`, without the paths of copied files that GPUI's own
/// `text` gives in its place, if it fits [`TEXT`].
fn plain_text(item: &ClipboardItem) -> Option<String> {
    let mut text = String::new();
    for entry in item.entries() {
        let ClipboardEntry::String(string) = entry else {
            continue;
        };
        let string = string.text();
        // Checked before it is copied.
        if string.len() > TEXT.saturating_sub(text.len()) {
            return None;
        }
        text.push_str(string);
    }
    Some(text)
}
