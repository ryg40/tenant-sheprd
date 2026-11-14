//! The reader an SSH host runs, here under this computer's `python3` with a
//! temporary home: it counts as the local reader does, it stops at 5,000
//! entries, it skips a record it cannot send and reads a home that is a
//! symlink, its exit status 4 is "not in time", it sends the token only to a
//! server whose `/health` names the record's `pid`, and a page address that
//! is not loopback, names another session, or comes through another local
//! port is refused. This computer reads the address of a page with the same
//! reader.
use super::waiting_count::{assert_counts_open_only, home, record, write};
use crate::{
    teleport::Host,
    walkr::waiting::{Error, remote},
};
use sheprd_client::ConnectTarget;
use std::{
    fs,
    io::{Read as _, Write as _},
    net::TcpListener,
    os::unix::fs::PermissionsExt as _,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const SESSION: &str = "20261103T120000Z-0123456789ab";

/// A host whose scripts run here, with `home` as its home.
fn host(home: &Path) -> Host {
    let mut host = Host::new(&ConnectTarget::Local).unwrap();
    host.env = vec![("HOME".into(), home.display().to_string())];
    host
}

#[test]
fn a_host_reader_counts_open_sessions_and_skips_refused_records() {
    let (home, _elsewhere) = home();
    let read = remote::list(
        &host(home.path()),
        remote::DEADLINE,
        &AtomicBool::new(false),
    );
    assert_counts_open_only(&read.unwrap());
}

/// The reader stops after 5,000 entries of the folder: more open sessions
/// than that would be more rows than Sheprd takes, and exactly 5,000 rows
/// are a full list.
#[test]
fn more_than_5000_open_sessions_count_as_5000() {
    let home = tempfile::tempdir().unwrap();
    for at in 0..5_200_u64 {
        let id = format!("20261103T120000Z-{at:012x}");
        write(home.path(), &id, record(&id, "open", "x").as_bytes());
    }
    let read = remote::list(
        &host(home.path()),
        remote::DEADLINE,
        &AtomicBool::new(false),
    );
    assert_eq!(read.unwrap().waiting, 5_000);
}

/// A title that has no UTF-8 form (a lone surrogate) skips its record only.
#[test]
fn a_record_with_a_lone_surrogate_is_skipped_and_the_host_is_read() {
    let (home, _elsewhere) = home();
    let id = "20261107T120000Z-0123456789ab";
    let bad = record(id, "open", "x").replace("\"Title ", "\"\\ud83d Title ");
    write(home.path(), id, bad.as_bytes());
    let read = remote::list(
        &host(home.path()),
        remote::DEADLINE,
        &AtomicBool::new(false),
    );
    assert_counts_open_only(&read.unwrap());
}

/// The home itself may be a symlink; the folders below it may not.
#[test]
fn a_home_that_is_a_symlink_is_read() {
    let (home, _elsewhere) = home();
    let links = tempfile::tempdir().unwrap();
    let link = links.path().join("home");
    std::os::unix::fs::symlink(home.path(), &link).unwrap();
    let read = remote::list(&host(&link), remote::DEADLINE, &AtomicBool::new(false));
    assert_counts_open_only(&read.unwrap());
}

/// Exit status 4, the reader's `TIMED_OUT`, is "not in time". The fake
/// `python3` never runs the reader, so this pins only the mapping.
#[test]
fn exit_status_4_maps_to_timed_out() {
    let home = tempfile::tempdir().unwrap();
    fake_python(home.path(), "exit 4");
    let read = remote::list(
        &host(home.path()),
        remote::DEADLINE,
        &AtomicBool::new(false),
    );
    assert!(matches!(read, Err(Error::TimedOut)), "{read:?}");
}

/// Puts a `python3` that runs `body` first on the host's `PATH`, which
/// finds `~/.local/bin` first.
fn fake_python(home: &Path, body: &str) {
    let bin = home.join(".local/bin");
    fs::create_dir_all(&bin).unwrap();
    let python = bin.join("python3");
    fs::write(&python, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&python, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Sheprd's own watchdog: output on standard error does not keep a read
/// alive past its deadline. The fake `python3` never runs the reader.
#[test]
fn endless_standard_error_ends_at_the_deadline() {
    let home = tempfile::tempdir().unwrap();
    fake_python(home.path(), "while :; do echo noise >&2; done");
    let deadline = Duration::from_secs(2);
    let started = Instant::now();
    let read = remote::list(&host(home.path()), deadline, &AtomicBool::new(false));
    assert!(matches!(read, Err(Error::TimedOut)), "{read:?}");
    assert!(started.elapsed() < deadline + Duration::from_secs(3));
}

#[test]
fn a_page_address_off_loopback_or_of_another_session_is_refused() {
    let answer = |address: &str| format!(r#"{{"page":"{address}"}}"#);
    let page = |host: &str, session: &str| {
        answer(&format!(
            "http://{host}/_walkr/compare/g/0123abcd#{session}"
        ))
    };
    let other = "20261104T120000Z-0123456789ab";
    for refused in [
        page("192.0.2.7:4100", SESSION),
        page("walkr.example.test:4100", SESSION),
        page("127.0.0.1", SESSION),
        page("127.0.0.1:4100", other),
        answer("https://127.0.0.1:4100/_walkr/compare/g/0123abcd#20261103T120000Z-0123456789ab"),
    ] {
        let parsed = remote::parse_page(refused.as_bytes(), SESSION);
        assert!(
            matches!(parsed, Err(Error::PageRefused)),
            "{refused}: {parsed:?}"
        );
    }
    let accepted = remote::parse_page(page("127.0.0.1:4100", SESSION).as_bytes(), SESSION).unwrap();
    assert_eq!(accepted.port(), 4100);
    let local = accepted.tunneled(4100).unwrap();
    assert_eq!(local.session(), Some(SESSION));
    assert_eq!(local.host(), "127.0.0.1:4100");
}

/// The server answers only its own port in `Host`, so a tunnel that could
/// not take that port on this computer opens no page.
#[test]
fn a_tunnel_on_another_local_port_opens_no_page() {
    let answer = r#"{"page":"http://127.0.0.1:4100/_walkr/compare/g/0123abcd#20261103T120000Z-0123456789ab"}"#;
    let page = remote::parse_page(answer.as_bytes(), SESSION).unwrap();
    let busy = page.tunneled(4200);
    assert!(matches!(busy, Err(Error::PortBusy(4100))), "{busy:?}");
}

/// A home without the record of a Walkr server gives "no server", and no
/// page address.
#[test]
fn a_home_without_a_server_record_gives_no_server() {
    let empty = tempfile::tempdir().unwrap();
    let read = remote::page(
        &host(empty.path()),
        SESSION,
        remote::DEADLINE,
        &AtomicBool::new(false),
    );
    assert!(matches!(read, Err(Error::NoServer)), "{read:?}");
}

/// The token goes only to a server whose `/health` answer names the `pid` of
/// `server.json`: another `pid`, or none, ends the read with "another
/// server" before any request carries the token. The listener is this
/// test's own, on loopback; a matching `pid` shows that it sees the token.
#[test]
fn the_token_goes_only_to_a_server_whose_health_names_the_records_pid() {
    let token_sent = |health: &str| {
        let home = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let record = home.path().join(".human-review");
        fs::create_dir(&record).unwrap();
        let server = format!(r#"{{"port":{port},"token":"secret-token","protocol":9,"pid":4242}}"#);
        fs::write(record.join("server.json"), server).unwrap();
        let done = Arc::new(AtomicBool::new(false));
        let serving = {
            let (done, health) = (Arc::clone(&done), health.to_owned());
            thread::spawn(move || serve(&listener, &done, &health))
        };
        let read = remote::page(
            &host(home.path()),
            SESSION,
            remote::DEADLINE,
            &AtomicBool::new(false),
        );
        done.store(true, Ordering::Relaxed);
        let requests = serving.join().unwrap();
        assert!(
            matches!(read, Err(Error::OtherServer)),
            "{health}: {read:?}"
        );
        assert!(requests[0].starts_with("get /health "), "{requests:?}");
        requests
            .iter()
            .any(|request| request.contains("x-human-review-token: secret-token"))
    };
    assert!(!token_sent(r#"{"ok":true,"protocol":9,"pid":4343}"#));
    assert!(!token_sent(r#"{"ok":true,"protocol":9}"#));
    assert!(!token_sent(r#"{"ok":true,"protocol":9,"pid":4242.0}"#));
    assert!(token_sent(r#"{"ok":true,"protocol":9,"pid":4242}"#));
}

/// Answers `health` on `/health` and 404 elsewhere until `done`, and gives
/// back each request's head in lower case.
fn serve(listener: &TcpListener, done: &AtomicBool, health: &str) -> Vec<String> {
    let mut requests = Vec::new();
    while !done.load(Ordering::Relaxed) {
        let Ok((mut stream, _)) = listener.accept() else {
            thread::sleep(Duration::from_millis(5));
            continue;
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut head = Vec::new();
        let mut byte = [0];
        while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap() == 1 {
            head.push(byte[0]);
        }
        let head = String::from_utf8(head).unwrap().to_lowercase();
        let (status, body) = if head.starts_with("get /health ") {
            ("200 OK", health)
        } else {
            ("404 Not Found", "{}")
        };
        let answer = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(answer.as_bytes()).unwrap();
        requests.push(head);
    }
    requests
}
