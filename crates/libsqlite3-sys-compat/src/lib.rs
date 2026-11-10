//! Stand-in for `libsqlite3-sys` 0.36, which Servo's `rusqlite` 0.38 asks
//! for.
//!
//! The workspace's own `rusqlite` 0.40 links `libsqlite3-sys` 0.38, and Cargo
//! allows one crate with `links = "sqlite3"` in a build. This package has no
//! `links` key of its own: it hands Servo the 0.38 bindings, so one bundled
//! SQLite serves both. Only a build with `sheprd-gpui`'s `servo` feature
//! compiles it.

pub use libsqlite3_sys_upstream::*;
