//! The program's answers are untrusted: a malformed, oversized, or unknown
//! answer is an error value, and finding the program runs nothing.
use super::*;

#[test]
fn a_known_answer_parses() {
    let status =
        program::parse_status(br#"{"state": "stopped", "url": "http://127.0.0.1:6790/browse"}"#)
            .unwrap();
    assert_eq!(status.state, State::Stopped);
    assert_eq!(status.url, Some(url("http://127.0.0.1:6790/browse")));
    let unset = program::parse_status(b"{\"state\": \"unconfigured\", \"url\": null}\n").unwrap();
    assert_eq!(unset.url, None);
}

#[test]
fn malformed_oversized_and_unknown_answers_are_errors() {
    for answer in [
        &b""[..],
        b"state: running",
        br#"{"state": "starting", "url": null}"#,
        br#"{"state": 3}"#,
        br#"{"url": "http://127.0.0.1:6790/browse"}"#,
        br#"[{"state": "running"}]"#,
        br#"{"state": "running", "url": "http://127.0.0.1:6790/"} {"state": "stopped"}"#,
    ] {
        assert!(
            matches!(program::parse_status(answer), Err(Error::Answer(_))),
            "{}",
            String::from_utf8_lossy(answer)
        );
    }
    for page in ["file:///etc/passwd", "javascript:alert(1)", "not a url"] {
        let answer = format!(r#"{{"state": "running", "url": "{page}"}}"#);
        assert!(
            matches!(
                program::parse_status(answer.as_bytes()),
                Err(Error::Address(_))
            ),
            "{page}"
        );
    }
    let mut oversized = br#"{"state": "running", "url": null, "pad": ""#.to_vec();
    oversized.resize(program::MAX_ANSWER + 1, b'x');
    assert!(matches!(
        program::parse_status(&oversized),
        Err(Error::Oversized { .. })
    ));
}

#[test]
fn a_program_reason_is_one_bounded_line() {
    let long = format!(
        "log line\n\nartifacts: port 6790 \u{202e}is in use{}\n",
        "x".repeat(400)
    );
    let line = error::reason_line(&long).unwrap();
    assert!(line.starts_with("port 6790 is in use"));
    assert!(!line.contains('\n') && !line.contains('\u{202e}'));
    assert_eq!(line.chars().count(), error::REASON_CHARS);
    assert_eq!(error::reason_line(" \n"), None);
}

#[cfg(unix)]
#[test]
fn the_program_is_found_on_path_then_in_local_bin() {
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join(".local").join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let on_path = tempfile::tempdir().unwrap();
    let empty = tempfile::tempdir().unwrap();
    assert_eq!(program::locate_in(None, Some(home.path())), None);
    let local = fake_program(&bin, "{}");
    assert_eq!(
        program::locate_in(Some(empty.path().as_os_str()), Some(home.path())),
        Some(local)
    );
    let listed = fake_program(on_path.path(), "{}");
    let path = std::env::join_paths([empty.path(), on_path.path()]).unwrap();
    assert_eq!(
        program::locate_in(Some(&path), Some(home.path())),
        Some(listed)
    );
    // A relative directory is never searched, nor a file without the
    // executable bit.
    assert_eq!(
        program::locate_in(Some(std::ffi::OsStr::new(".")), None),
        None
    );
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(
        bin.join(program::NAME),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert_eq!(program::locate_in(None, Some(home.path())), None);
}

#[cfg(unix)]
#[test]
fn a_fake_program_answers_and_its_failure_keeps_a_reason() {
    let dir = tempfile::tempdir().unwrap();
    let path = fake_program(
        dir.path(),
        r#"{"state": "running", "url": "http://127.0.0.1:6790/browse"}"#,
    );
    let status = program::status(&path).unwrap();
    assert_eq!(status.state, State::Running);
    assert!(!dir.path().join("started").exists());
    assert_eq!(
        program::start(&path).unwrap(),
        Some(url("http://127.0.0.1:6790/browse"))
    );
    assert!(dir.path().join("started").exists());

    // A program that went away is an error value, not a panic.
    assert!(matches!(
        program::status(&dir.path().join("missing")),
        Err(Error::Run(_))
    ));
    let failing = dir.path().join("failing");
    std::fs::write(
        &failing,
        "#!/bin/sh\necho 'artifacts: port 6790 is in use by another program' >&2\nexit 2\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(&failing, std::fs::Permissions::from_mode(0o755)).unwrap();
    let error = program::start(&failing).unwrap_err();
    assert_eq!(
        error.program_reason().as_deref(),
        Some("port 6790 is in use by another program")
    );
}
