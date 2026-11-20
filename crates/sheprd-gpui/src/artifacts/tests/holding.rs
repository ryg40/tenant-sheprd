//! Which rows of the port listing the stop question for the page's port
//! names: only those whose address would make a bind of the probed
//! address, IPv4 loopback, fail.
#[cfg(unix)]
use crate::local_ports::scan;
use crate::{
    artifacts::port::holding, listening_ports::Bound, local_ports::Listener, processes::Identity,
};
use std::num::NonZeroU16;

const PORT: u16 = 6123;

/// A row of process `pid` listening on [`PORT`] at each of `addresses`,
/// with `*` for the socket of every address.
fn row(pid: u32, addresses: &[&str]) -> Listener {
    let bound: Vec<Bound> = addresses
        .iter()
        .map(|address| match *address {
            "*" => Bound::Every,
            address => Bound::Address(address.parse().unwrap()),
        })
        .collect();
    Listener {
        identity: Identity { pid, started: 10 },
        birth: None,
        number: PORT,
        bind: bound[0].into(),
        bound,
        process: "python3".into(),
        name: "python3".into(),
        command: "python3 -m http.server".into(),
        directory: None,
        project: None,
        cpu: None,
        memory: 0,
        system: false,
        paused: false,
        started: None,
    }
}

fn named(listing: &[Listener]) -> Vec<u32> {
    holding(listing, NonZeroU16::new(PORT).unwrap())
        .iter()
        .map(|listener| listener.identity.pid)
        .collect()
}

#[test]
fn a_listener_on_another_loopback_address_is_left_out() {
    let listing = [
        row(1, &["127.0.0.2"]),
        row(2, &["::1"]),
        // An IPv6 wildcard that takes no IPv4 connections.
        row(3, &["::"]),
    ];
    assert_eq!(named(&listing), Vec::<u32>::new());
}

#[test]
fn a_listener_on_the_probed_address_or_a_wildcard_is_kept() {
    let listing = [
        row(1, &["127.0.0.1"]),
        row(2, &["0.0.0.0"]),
        row(3, &["*"]),
        // One row for a process on both loopbacks.
        row(4, &["::1", "127.0.0.1"]),
        row(5, &["127.0.0.2"]),
    ];
    assert_eq!(named(&listing), [1, 2, 3, 4]);
}

// The port listing is read only on Unix.
#[cfg(unix)]
#[test]
fn a_listener_bound_to_a_device_is_named_from_the_listing_s_own_text() {
    // `ss -ltnp` of iproute2 6.17 for listeners on one number: an IPv6
    // socket at IPv4's loopback, bound to the device `lo`; the same address
    // bound to another device by a second process, which takes no
    // connection to the loopback but makes a bind of it fail all the same;
    // and another address of the loopback network.
    let text = "ss\n\
State  Recv-Q Send-Q             Local Address:Port  Peer Address:PortProcess\n\
LISTEN 0      128        [::ffff:127.0.0.1]%lo:6123             *:*    users:((\"python3\",pid=41,fd=7))\n\
LISTEN 0      128    [::ffff:127.0.0.1]%wlp2s0:6123             *:*    users:((\"python3\",pid=43,fd=8))\n\
LISTEN 0      128                    127.0.0.2:6123       0.0.0.0:*    users:((\"python3\",pid=42,fd=4))\n";
    let listing: Vec<Listener> = scan::parse(text)
        .unwrap()
        .into_iter()
        .map(|socket| Listener {
            number: socket.number,
            bind: socket.bind,
            bound: socket.bound,
            ..row(socket.pid, &["*"])
        })
        .collect();
    let mut listed: Vec<u32> = listing.iter().map(|row| row.identity.pid).collect();
    listed.sort_unstable();
    assert_eq!(listed, [41, 42, 43]);
    let named = named(&listing);
    assert!(named.contains(&41) && named.contains(&43), "{named:?}");
    assert!(!named.contains(&42), "{named:?}");
}
