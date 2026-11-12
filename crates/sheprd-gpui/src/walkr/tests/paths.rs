use super::*;
use crate::browser::WebUrl;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

#[test]
fn a_double_slash_path_contacts_only_the_configured_host() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let other = TcpListener::bind("127.0.0.1:0").unwrap();
    let url =
        WebUrl::try_from(format!("http://{}", listener.local_addr().unwrap()).as_str()).unwrap();
    let path = format!("//{}/sdk.js", other.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok((mut stream, _)) = listener.accept() {
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut byte = [0];
                while !bytes.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    bytes.push(byte[0]);
                }
                write!(
                    stream,
                    "HTTP/1.1 204 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
                return bytes;
            }
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
    });
    let reach = super::super::probe::probe(
        &super::super::probe::agent(Duration::from_secs(1)),
        &url,
        &path,
    );
    assert_eq!(reach, Reach::Answers);
    assert!(
        server
            .join()
            .unwrap()
            .starts_with(format!("GET {path} HTTP/1.1\r\n").as_bytes())
    );
    other.set_nonblocking(true).unwrap();
    assert!(matches!(other.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock));
}
