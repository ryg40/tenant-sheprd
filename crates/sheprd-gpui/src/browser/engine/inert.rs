//! The engine a unit test gets, which no thread reads. It keeps, on the
//! test's thread, the address of each page it is asked to open, so that a
//! test checks what a window asked for rather than the window's own state.
use super::Command;
use std::cell::RefCell;

thread_local! {
    static OPENED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn sent(command: &Command) {
    if let Command::Open(open) = command {
        OPENED.with_borrow_mut(|opened| opened.push(open.url.clone()));
    }
}

/// The addresses of the pages asked for on this thread since the last call.
pub(in crate::browser) fn opened() -> Vec<String> {
    OPENED.with_borrow_mut(std::mem::take)
}
