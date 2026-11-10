#![allow(clippy::unwrap_used)]
use super::*;

/// More requests than the answers' channel holds.
const REQUESTS: u64 = 4 * ANSWERS as u64;

fn asked() -> (Asked<u64>, Receiver<(u64, Answer)>) {
    let (answers, answered) = crossbeam_channel::bounded(ANSWERS);
    let (wakes, _) = crossbeam_channel::bounded(1);
    (Asked::new(answers, Wake(wakes)), answered)
}

fn answer(status: u16) -> Answer {
    Answer {
        status,
        content_type: "text/plain",
        body: Vec::new(),
    }
}

/// Each request stands for itself, so an answer is known to reach the
/// request it was given for.
fn settled(asked: &Asked<u64>) -> Vec<(u64, u16)> {
    let answered = RefCell::new(Vec::new());
    asked.settle(|load, answer| answered.borrow_mut().push((load, answer.status)));
    answered.into_inner()
}

#[test]
fn every_answer_given_on_the_engines_thread_reaches_its_request() {
    let (asked, answered) = asked();
    let responders: Vec<_> = (0..REQUESTS).map(|load| asked.wait(load)).collect();
    // Nothing reads the channel here: an answer that went through it would
    // be lost, or wait for ever, once the channel is full.
    for responder in responders {
        responder(answer(403));
    }
    let settled = settled(&asked);
    assert_eq!(
        settled,
        (0..REQUESTS).map(|load| (load, 403)).collect::<Vec<_>>()
    );
    assert!(answered.is_empty());
    assert!(asked.loads.borrow().is_empty());
}

#[test]
fn every_answer_of_the_worker_reaches_its_request() {
    let (asked, answered) = asked();
    let responders: Vec<_> = (0..REQUESTS).map(|load| asked.wait(load)).collect();
    // The worker answers faster than this thread reads: it waits for a
    // place rather than drop an answer.
    let worker = std::thread::spawn(move || {
        for responder in responders {
            responder(answer(404));
        }
    });
    let mut loads = Vec::new();
    for (number, answer) in answered.iter().take(REQUESTS as usize) {
        assert_eq!(answer.status, 404);
        loads.extend(asked.take(number));
    }
    worker.join().unwrap();
    loads.sort_unstable();
    assert_eq!(loads, (0..REQUESTS).collect::<Vec<_>>());
    assert!(settled(&asked).is_empty());
    assert!(asked.loads.borrow().is_empty());
}
