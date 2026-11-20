#![allow(clippy::unwrap_used)]
use super::*;

const LOST: Result<JSValue, JavaScriptEvaluationError> =
    Err(JavaScriptEvaluationError::InternalError);

/// Servo answers a page between two documents with the same internal error
/// as a page whose script thread is gone. Only a page that kept one
/// document from the question to its answer is told that it stopped; one
/// with no document yet, or whose next document started, is asked again as
/// its load completes.
#[test]
fn an_error_from_a_page_between_two_documents_does_not_stop_its_tab() {
    let mailbox = Arc::new(Mailbox::default());
    let probe = Probe::new(mailbox.clone(), Arc::default());
    probe.ended();
    // No first document yet: nothing to ask.
    assert!(probe.ask(false).is_none());
    probe.loaded(LoadStatus::Complete);
    let asked = probe.ask(true).unwrap();
    // The page starts its next document before it answers.
    probe.loaded(LoadStatus::Started);
    probe.answered(asked, &LOST);
    assert!(!mailbox.stopped.take());

    probe.loaded(LoadStatus::Complete);
    let asked = probe.ask(true).unwrap();
    probe.answered(asked, &LOST);
    assert!(mailbox.stopped.take());
}

/// A question waits until the page's script thread reads it, and a thread
/// that ends first drops it unanswered. The next end asks the page once
/// more: Servo cannot send that question to a thread that is gone, and its
/// error tells the page that it stopped. A page that answers nothing
/// through many ends has no more than `WAITING` questions waiting.
#[test]
fn a_page_whose_question_ended_with_its_thread_is_asked_at_the_next_end() {
    let mailbox = Arc::new(Mailbox::default());
    let probe = Probe::new(mailbox.clone(), Arc::default());
    probe.ended();
    // The page is busy, and its thread ends before it reads the question.
    probe.ask(true).unwrap();
    probe.ended();
    let asked = probe.ask(true).unwrap();
    probe.answered(asked, &LOST);
    assert!(mailbox.stopped.take());

    let probe = Probe::new(Arc::default(), Arc::default());
    let waiting = (0..usize::from(WAITING) * 3)
        .filter(|_| {
            probe.ended();
            probe.ask(true).is_some()
        })
        .count();
    assert!(waiting <= usize::from(WAITING));
}
