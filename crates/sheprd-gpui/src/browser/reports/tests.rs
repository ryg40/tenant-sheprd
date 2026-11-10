use super::*;

#[test]
fn a_run_of_dropped_reports_writes_the_first_and_then_few_lines() {
    assert!(summarized(1));
    let lines = (1..=1_000_000_u64)
        .filter(|dropped| summarized(*dropped))
        .count();
    assert_eq!(lines, 20);
}

#[test]
fn a_dropped_report_is_named_without_what_the_page_put_in_it() {
    let source = Source::Tab(TabId::test(7));
    let event = Event::Title(source, "a title with a secret".to_owned());
    assert_eq!(event.origin(), ("title", &source));
}
