//! The status answer and the setting decide what a click does, and no
//! answer starts the server: a stopped one is only asked about. The setting
//! wins unless the program serves its origin.
use super::*;

fn status(state: State, page: Option<&str>) -> Status {
    Status {
        state,
        url: page.map(url),
    }
}

#[test]
fn running_opens_stopped_asks_and_the_rest_refuse() {
    let config = Config::default();
    let page = Some("http://127.0.0.1:6790/browse");
    let browse = || url("http://127.0.0.1:6790/browse");
    assert_eq!(
        decide(Some(&status(State::Running, page)), &config),
        Some(Action::Open(browse()))
    );
    assert_eq!(
        decide(Some(&status(State::Remote, page)), &config),
        Some(Action::Open(browse()))
    );
    assert_eq!(
        decide(Some(&status(State::Stopped, page)), &config),
        Some(Action::Ask(browse()))
    );
    assert_eq!(
        decide(Some(&status(State::Unreachable, page)), &config),
        Some(Action::Refuse(Refusal::Unreachable))
    );
    assert_eq!(
        decide(Some(&status(State::Unconfigured, None)), &config),
        Some(Action::Refuse(Refusal::Unconfigured))
    );
    assert_eq!(
        decide(Some(&status(State::Running, None)), &config),
        Some(Action::Refuse(Refusal::NoAddress))
    );
}

#[test]
fn a_setting_of_the_programs_origin_is_the_page_asked_about() {
    let config = Config {
        url: Some(url("http://127.0.0.1:6790/")),
    };
    let program = Some("http://127.0.0.1:6790/browse");
    assert_eq!(
        decide(Some(&status(State::Stopped, program)), &config),
        Some(Action::Ask(url("http://127.0.0.1:6790/")))
    );
    assert_eq!(
        decide(Some(&status(State::Running, program)), &config),
        Some(Action::Open(url("http://127.0.0.1:6790/")))
    );
    // `localhost` is the same local server as `127.0.0.1`.
    let config = Config {
        url: Some(url("http://localhost:6790/")),
    };
    assert_eq!(
        decide(Some(&status(State::Stopped, program)), &config),
        Some(Action::Ask(url("http://localhost:6790/")))
    );
}

#[test]
fn a_setting_of_another_origin_opens_and_asks_nothing() {
    let setting = || url("https://artifacts.example.com/");
    let config = Config {
        url: Some(setting()),
    };
    // Another port is another origin, so the program does not decide.
    for program in [
        "http://127.0.0.1:6790/browse",
        "https://artifacts.example.com:8443/",
    ] {
        for state in [State::Stopped, State::Running, State::Remote] {
            assert_eq!(
                decide(Some(&status(state, Some(program))), &config),
                Some(Action::Open(setting())),
                "{state:?} at {program}"
            );
        }
    }
}

#[test]
fn start_asks_about_a_stopped_server_that_the_click_leaves_alone() {
    let config = Config {
        url: Some(url("https://artifacts.example.com/")),
    };
    let stopped = status(State::Stopped, Some("http://127.0.0.1:6790/browse"));
    assert_eq!(
        decide_start(&stopped, &config),
        Some(Action::Ask(url("http://127.0.0.1:6790/browse")))
    );
    assert_eq!(
        decide(Some(&stopped), &config),
        Some(Action::Open(url("https://artifacts.example.com/")))
    );
    // A setting of the program's server is the page asked about, as a click.
    let config = Config {
        url: Some(url("http://localhost:6790/")),
    };
    assert_eq!(
        decide_start(&stopped, &config),
        Some(Action::Ask(url("http://localhost:6790/")))
    );
    // Any other state follows the click: a running server is not asked about.
    let config = Config {
        url: Some(url("https://artifacts.example.com/")),
    };
    let running = status(State::Running, Some("http://127.0.0.1:6790/browse"));
    assert_eq!(
        decide_start(&running, &config),
        Some(Action::Open(url("https://artifacts.example.com/")))
    );
}

#[test]
fn a_failed_or_empty_answer_opens_the_setting() {
    let setting = || url("http://127.0.0.1:6790/");
    let config = Config {
        url: Some(setting()),
    };
    let same = Some("http://127.0.0.1:6790/browse");
    assert_eq!(decide(None, &config), Some(Action::Open(setting())));
    assert_eq!(
        decide(Some(&status(State::Unreachable, same)), &config),
        Some(Action::Open(setting()))
    );
    assert_eq!(
        decide(Some(&status(State::Unconfigured, None)), &config),
        Some(Action::Open(setting()))
    );
    // Without a setting a failed read opens nothing; its error is shown.
    assert_eq!(decide(None, &Config::default()), None);
}

#[test]
fn only_a_running_local_server_is_asked_about_stopping() {
    assert_eq!(decide_stop(State::Running), Stop::Ask);
    assert_eq!(decide_stop(State::Unreachable), Stop::Ask);
    assert_eq!(decide_stop(State::Stopped), Stop::NothingRuns);
    assert_eq!(decide_stop(State::Remote), Stop::Remote);
    assert_eq!(decide_stop(State::Unconfigured), Stop::Unconfigured);
}
