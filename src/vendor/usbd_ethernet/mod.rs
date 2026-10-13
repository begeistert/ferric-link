// Vendored from usbd-ethernet 0.4.0 (MIT, Daniel Kenyon-Jones, https://github.com/dlkj/usbd-ethernet),
// plus the Apple fix that no release has. See `src/vendor/README.md`.
#![allow(dead_code, clippy::all, clippy::pedantic)]

mod buffer;
mod bytes;
mod ethernet;

pub use ethernet::*;
