use super::*;

/// Places a run of `count` records of one kind finds at `now`.
fn placed(budget: &mut Budget, error: bool, count: u32, now: Instant) -> Vec<Place> {
    (0..count)
        .filter_map(|_| budget.admit(error, now))
        .collect()
}

fn count(places: &[Place]) -> u32 {
    u32::try_from(places.len()).unwrap_or(u32::MAX)
}

#[test]
fn a_flood_stays_within_the_tabs_burst_and_says_once_that_it_is_spent() {
    let (mut budget, now) = (Budget::default(), Instant::now());
    let places = placed(&mut budget, false, 10 * BURST, now);
    assert_eq!(count(&places), BURST);
    assert!(places.iter().all(|place| place.skipped == 0));
    let spent: Vec<bool> = places.iter().map(|place| place.spent).collect();
    assert_eq!(spent.iter().filter(|spent| **spent).count(), 1);
    assert_eq!(spent.last(), Some(&true));
}

#[test]
fn time_gives_places_back_one_at_a_time_and_the_next_record_counts_the_skipped() {
    let (mut budget, start) = (Budget::default(), Instant::now());
    placed(&mut budget, false, 10 * BURST, start);
    // Not yet a whole interval.
    assert_eq!(budget.admit(false, start + REFILL / 2), None);
    let later = start + REFILL * 3 + REFILL / 2;
    let places = placed(&mut budget, false, 10, later);
    assert_eq!(
        places.first(),
        Some(&Place {
            skipped: 9 * BURST + 1,
            spent: false,
        })
    );
    assert_eq!(count(&places), 3);
    // The half interval is not lost: one more half completes a place.
    assert!(budget.admit(false, later + REFILL / 2).is_some());
}

#[test]
fn a_page_that_keeps_flooding_gets_one_record_an_interval() {
    let (mut budget, start) = (Budget::default(), Instant::now());
    let intervals = 1000;
    let kept: u32 = (0..=intervals)
        .map(|interval| count(&placed(&mut budget, false, 3, start + REFILL * interval)))
        .sum();
    assert_eq!(kept, BURST + intervals);
}

#[test]
fn a_long_silence_gives_no_more_than_the_burst() {
    let (mut budget, start) = (Budget::default(), Instant::now());
    placed(&mut budget, true, 10 * BURST, start);
    let later = start + REFILL * (100 * BURST);
    assert_eq!(count(&placed(&mut budget, false, 10 * BURST, later)), BURST);
    assert_eq!(count(&placed(&mut budget, true, 10 * BURST, later)), ERRORS);
}

#[test]
fn an_error_after_a_flood_of_other_lines_has_a_place() {
    let (mut budget, now) = (Budget::default(), Instant::now());
    assert_eq!(count(&placed(&mut budget, false, 300, now)), BURST);
    assert_eq!(
        budget.admit(true, now),
        Some(Place {
            skipped: 300 - BURST,
            spent: false,
        })
    );
    // Errors have a bound of their own, and other lines cannot use it.
    assert_eq!(
        count(&placed(&mut budget, true, 10 * ERRORS, now)),
        ERRORS - 1
    );
    assert_eq!(budget.admit(false, now), None);
}

#[test]
fn a_flood_of_errors_uses_its_own_places_and_then_the_common_ones() {
    let (mut budget, now) = (Budget::default(), Instant::now());
    let places = placed(&mut budget, true, 10 * (BURST + ERRORS), now);
    assert_eq!(count(&places), BURST + ERRORS);
    assert_eq!(places.iter().filter(|place| place.spent).count(), 1);
    assert_eq!(budget.admit(false, now), None);
}

#[test]
fn a_load_the_user_asks_for_gives_every_place_again() {
    let (mut budget, now) = (Budget::default(), Instant::now());
    placed(&mut budget, true, 10 * (BURST + ERRORS), now);
    budget.renew();
    let places = placed(&mut budget, true, 10 * (BURST + ERRORS), now);
    assert_eq!(count(&places), BURST + ERRORS);
    // What was skipped before the load is still reported.
    assert_ne!(places.first().map(|place| place.skipped), Some(0));
}

#[test]
fn a_clock_that_steps_back_gives_nothing_and_takes_nothing() {
    let (mut budget, start) = (Budget::default(), Instant::now() + REFILL * 10);
    placed(&mut budget, false, 10 * BURST, start);
    assert_eq!(budget.admit(false, start - REFILL * 5), None);
    assert!(budget.admit(false, start + REFILL).is_some());
}

#[test]
fn a_short_message_is_whole_and_a_long_one_ends_on_a_character() {
    assert_eq!(bounded("ready"), ("ready", false));
    // Two bytes each, and one byte before them: the cut falls inside one.
    let long = format!("a{}", "é".repeat(LINE_BYTES));
    let (text, cut) = bounded(&long);
    assert_eq!((text.len(), cut), (LINE_BYTES - 1, true));
}

#[test]
fn an_address_loses_its_query_fragment_and_user_but_keeps_its_position() {
    for (text, expected) in [
        (
            "Error at https://h/p?code=1#f:1:2 x",
            "Error at https://h/p:1:2 x",
        ),
        (
            "Error at https://h/cb?code=4f9a&state=s:12:345 ReferenceError: y",
            "Error at https://h/cb:12:345 ReferenceError: y",
        ),
        ("got https://h/p#access_token=abc", "got https://h/p"),
        ("https://u:pw@h:8443/p?x=1 and", "https://h:8443/p and"),
        (
            "a http://h?next=https://o/cb?code=2 b wss://k/#t c",
            "a http://h b wss://k/ c",
        ),
        // A query's end is the next white space, whatever stands before it.
        (
            "fetch(\"https://h/p?x=1\") failed",
            "fetch(\"https://h/p failed",
        ),
        ("é https://h/é?é=é:3 é", "é https://h/é:3 é"),
        // A path without a host, as a page logs its own requests.
        (
            "GET /api/v3/flows/?query=client_id%3Dabc%26state%3Ds 200",
            "GET /api/v3/flows/ 200",
        ),
        ("/cb?code=4f9a#f:12:345 y", "/cb:12:345 y"),
        (
            "fetch('/a/b#top?code=1') //cdn.example/x?k=1",
            "fetch('/a/b //cdn.example/x",
        ),
        ("see a/b,/é?é=é\nnext", "see a/b,/é\nnext"),
    ] {
        assert_eq!(without_queries(text), expected, "{text}");
    }
}

#[test]
fn text_without_a_query_is_not_changed() {
    for text in [
        "ready",
        "Error at herdr-preview://localhost/chunk.js:2:18 x is not defined",
        "https://h:8443/a/b.html",
        "a :// b ://",
        "data:text/html,<p>?x#y",
        "either/or? 1 / 2 ? 3",
        // A letter outside ASCII before the `/` is a letter too.
        "été/hiver? oui",
        // A fragment stays when its path holds no `?`.
        "GET /api/v3/flows/#top 200 what?",
    ] {
        assert_eq!(without_queries(text), text);
    }
}
