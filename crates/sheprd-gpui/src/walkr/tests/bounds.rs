use super::*;
use crate::browser::WebUrl;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

#[test]
fn configuration_changes_keep_one_probe_in_flight() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let config = WalkrConfig {
        url: Some(
            WebUrl::try_from(format!("http://{}", listener.local_addr().unwrap()).as_str())
                .unwrap(),
        ),
        ..WalkrConfig::default()
    };
    let (accepted, requests) = mpsc::sync_channel(1);
    let server = thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        first
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut byte = [0];
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            first.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        first.set_nonblocking(true).unwrap();
        accepted.send(()).unwrap();
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(7);
        loop {
            match listener.accept() {
                Ok((mut next, _)) => {
                    // A second accepted connection must follow EOF on the stalled one.
                    let concurrent = !matches!(first.read(&mut byte), Ok(0));
                    next.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                    let mut request = Vec::new();
                    while !request.ends_with(b"\r\n\r\n") {
                        next.read_exact(&mut byte).unwrap();
                        request.push(byte[0]);
                    }
                    write!(
                        next,
                        "HTTP/1.1 204 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .unwrap();
                    return (concurrent, request, listener.accept().is_ok());
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline);
                    thread::yield_now();
                }
                Err(error) => panic!("{error}"),
            }
        }
    });
    let now = Instant::now();
    let mut walkr = Walkr::default();
    walkr.poll(true, &config, now);
    requests.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut latest = config.clone();
    for path in ["/a.js", "/b.js", "/a.js", "/latest.js"] {
        latest.probe_path = path.into();
        walkr.poll(true, &latest, now);
    }
    let update = walkr
        .worker
        .as_ref()
        .unwrap()
        .updates
        .recv_timeout(Duration::from_secs(6))
        .unwrap();
    assert!(!walkr.apply(update));
    let update = walkr
        .worker
        .as_ref()
        .unwrap()
        .updates
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    assert!(walkr.apply(update));
    assert_eq!(walkr.connectivity(), Connectivity::Reachable);
    let (concurrent, request, extra) = server.join().unwrap();
    assert!(!concurrent && !extra);
    assert!(request.starts_with(b"GET /latest.js HTTP/1.1\r\n"));
}

#[test]
fn an_old_generation_is_dropped_even_after_a_to_b_to_a() {
    let now = Instant::now();
    let a = WalkrConfig {
        url: Some(WebUrl::try_from("https://walkr.test").unwrap()),
        ..WalkrConfig::default()
    };
    let mut b = a.clone();
    b.probe_path = "/other.js".into();
    let mut walkr = Walkr::default();
    walkr.poll(false, &a, now);
    let old = walkr.generation;
    walkr.poll(false, &b, now);
    for config in [&b, &a] {
        walkr.poll(false, config, now);
        assert!(!walkr.apply(Update {
            generation: old,
            reach: Reach::Answers,
            checked: now,
        }));
        assert_eq!(walkr.connectivity(), Connectivity::Checking);
        assert_eq!(walkr.age(config, now), None);
    }
    assert!(walkr.apply(Update {
        generation: walkr.generation,
        reach: Reach::NetworkRefused,
        checked: now,
    }));
    assert_eq!(walkr.connectivity(), Connectivity::NetworkRefused);
}
