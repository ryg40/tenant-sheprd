//! The start time of a `/proc/<pid>/stat` record, whatever its process is
//! named.
use super::*;
use crate::local_ports::birth::ticks;

/// A stat record of a process named `name`, started at tick 2391855.
fn record(name: &[u8]) -> Vec<u8> {
    let mut record = b"3039655 (".to_vec();
    record.extend_from_slice(name);
    record.extend_from_slice(
        b") S 3039619 3039619 3039617 0 -1 4194304 79 0 0 0 0 0 0 0 20 0 1 0 2391855 \
          5767168 249 18446744073709551615\n",
    );
    record
}

#[test]
fn the_start_is_read_past_any_process_name() {
    // A name the kernel cut through a character, which is no UTF-8.
    assert_eq!(ticks(&record(b"serveur-d\xc3")), Some(Birth(2_391_855)));
    assert_eq!(ticks(&record(b"a) (b 7 8")), Some(Birth(2_391_855)));
}
