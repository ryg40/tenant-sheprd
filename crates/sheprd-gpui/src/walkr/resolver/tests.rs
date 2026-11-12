#![allow(clippy::unwrap_used)]
use super::*;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

#[test]
fn a_stalled_lookup_refuses_repeated_requests_without_more_lookups() {
    let (entered, started) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let service = Service::start(move |_, _| {
        count.fetch_add(1, Ordering::SeqCst);
        entered.send(()).unwrap();
        released.recv_timeout(Duration::from_secs(3)).unwrap();
        Err(ureq::Error::HostNotFound)
    })
    .unwrap();
    let dns = Dns(Arc::new(Mutex::new(Some(service))));
    let uri: Uri = "http://lookup.test/sdk.js".parse().unwrap();
    let config = Config::default();
    let timeout = NextTimeout {
        after: Duration::from_millis(100).into(),
        ..NextTimeout::default()
    };
    let probe = dns.clone();
    let first_uri = uri.clone();
    let first_config = config.clone();
    let first = thread::spawn(move || probe.resolve(&first_uri, &first_config, timeout));
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(
        first.join().unwrap(),
        Err(ureq::Error::Timeout(_))
    ));
    for _ in 0..8 {
        assert!(matches!(
            dns.resolve(&uri, &config, timeout),
            Err(ureq::Error::Timeout(_))
        ));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    release.send(()).unwrap();
    let state = dns.0.lock().unwrap();
    assert!(matches!(
        state
            .as_ref()
            .unwrap()
            .replies
            .recv_timeout(Duration::from_secs(2))
            .unwrap(),
        Err(ureq::Error::HostNotFound)
    ));
}
