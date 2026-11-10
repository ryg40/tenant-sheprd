//! Stand-in for `freetype-sys` 0.20, which `zed-font-kit` asks for on Linux.
//!
//! Servo (the `servo` feature of `sheprd-gpui`) links `freetype-sys` 0.23,
//! and Cargo allows one crate with `links = "freetype"` in a build. This
//! package has no `links` key of its own: it hands font-kit the 0.23
//! bindings, so one native FreeType serves both.
//!
//! Empty on macOS and Windows, where nothing asks for FreeType and the
//! workspace still builds this member.

#[cfg(not(any(target_os = "macos", windows)))]
pub use freetype_sys_upstream::*;
