use super::super::probe::{agent, probe};
use super::*;
use crate::browser::WebUrl;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread,
};

fn request(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut byte = [0];
    while !bytes.ends_with(b"\r\n\r\n") && bytes.len() < 8192 {
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
    }
    String::from_utf8(bytes).unwrap()
}

fn response(status: u16) -> (Reach, String, bool) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url =
        WebUrl::try_from(format!("http://{}/ignored", listener.local_addr().unwrap()).as_str())
            .unwrap();
    let (done, finished) = mpsc::sync_channel(1);
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = request(&mut stream);
        write!(stream, "HTTP/1.1 {status} Test\r\nLocation: /elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        drop(stream);
        finished.recv_timeout(Duration::from_secs(2)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let extra = listener.accept().is_ok();
        (request, extra)
    });
    let reach = probe(&agent(Duration::from_millis(500)), &url, "/sdk.js");
    done.send(()).unwrap();
    let (request, extra) = server.join().unwrap();
    (reach, request, extra)
}

#[test]
fn local_statuses_and_redirects_use_one_credential_free_get() {
    for (status, expected) in [
        (403, Reach::NetworkRefused),
        (502, Reach::Down(502)),
        (302, Reach::Answers),
    ] {
        let (reach, request, extra) = response(status);
        assert_eq!(reach, expected);
        assert!(!extra);
        assert!(request.starts_with("GET /sdk.js HTTP/1.1\r\n"));
        let headers = request.to_ascii_lowercase();
        assert!(!headers.contains("\r\ncookie:"));
        assert!(!headers.contains("\r\nauthorization:"));
    }
}

#[test]
fn a_closed_port_is_unreachable() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url =
        WebUrl::try_from(format!("http://{}", listener.local_addr().unwrap()).as_str()).unwrap();
    drop(listener);
    assert_eq!(
        probe(&agent(Duration::from_millis(100)), &url, "/sdk.js"),
        Reach::Unreachable(Unreachable::Connect)
    );
}

#[test]
fn a_silent_listener_times_out_within_the_bound() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url =
        WebUrl::try_from(format!("http://{}", listener.local_addr().unwrap()).as_str()).unwrap();
    let (done, finished) = mpsc::sync_channel(1);
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        request(&mut stream);
        finished.recv_timeout(Duration::from_secs(2)).unwrap();
    });
    let started = Instant::now();
    let reach = probe(&agent(Duration::from_millis(100)), &url, "/sdk.js");
    done.send(()).unwrap();
    server.join().unwrap();
    assert_eq!(reach, Reach::Unreachable(Unreachable::Timeout));
    assert!(started.elapsed() < Duration::from_secs(1));
}
