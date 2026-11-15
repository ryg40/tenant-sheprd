//! `walkr clear` takes the waiting sessions of the window out of its count,
//! answers how many, and `walkr status` then counts the rest.
use super::*;
use crate::{
    automations::Machine,
    control::{protocol::walkr::Reply as WalkrReply, server::walkr::clear},
    sidebar::layout_tests::fixture_window,
    walkr::history::pending::Tally,
};

const OPEN: &str = "20261103T120000Z-0123456789ab";

fn cleared(response: &Response) -> Option<u32> {
    match response {
        Response::Walkr {
            reply: WalkrReply::Cleared { count },
        } => Some(*count),
        _ => None,
    }
}

#[gpui::test]
fn walkr_clear_answers_the_number_cleared(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    view.update(cx, |view, _| {
        view.walkr.waiting.test_answered(
            Machine::Local,
            "This computer",
            Tally {
                waiting: 1,
                open: vec![OPEN.into()],
                complete: true,
                ..Tally::default()
            },
        );
    });
    let response = cx.cx.update(|cx| clear(&Caller::default(), cx));
    assert_eq!(cleared(&response), Some(1), "{response:?}");
    view.read_with(cx, |view, _| {
        assert_eq!(view.walkr.waiting.total(), 0);
    });
    // Nothing is left to clear.
    let response = cx.cx.update(|cx| clear(&Caller::default(), cx));
    assert_eq!(cleared(&response), Some(0), "{response:?}");
}
