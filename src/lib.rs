//! # ferric-link
//!
//! The firmware side of **Ferric Link**, the console that Ferric
//! (a Rust IDE for iPad) opens to a Raspberry Pi Pico over USB.
//!
//! Firmware written in Ferric calls [`read_serial!`] to read what the user types in the IDE's console and
//! [`init_log`] to route the `log` crate there. This crate makes that code an ordinary Rust dependency, so a
//! project created in Ferric builds with plain `cargo build` anywhere.
//!
//! **This release is the no-op API.** [`read_serial!`] never reads anything (it returns `0`) and [`init_log`]
//! does nothing; the USB link itself (CDC-NCM with a TCP console) lands in a later release behind a `link`
//! feature, with the same calls. Code written against 0.1 keeps compiling.
//!
//! ```
//! let mut buf = [0u8; 64];
//! let n = ferric_link::read_serial!(&mut buf);
//! assert_eq!(n, 0); // nothing to read: no link in this build
//! ferric_link::init_log();
//! ```
#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Reads what the user typed in Ferric's console into a byte buffer and evaluates to the number of bytes read.
///
/// The argument is anything that borrows as `&mut [u8]` (a `&mut [u8; N]`, a `&mut [u8]`). It never blocks.
/// Without the link (this release, or a build where it is not enabled) it reads nothing and evaluates to `0usize`,
/// so a polling loop simply sees no input.
///
/// ```
/// let mut line = [0u8; 32];
/// if ferric_link::read_serial!(&mut line) > 0 {
///     // handle the input
/// }
/// ```
#[macro_export]
macro_rules! read_serial {
    ($buf:expr) => {
        $crate::__read_serial(&mut $buf[..])
    };
}

#[doc(hidden)]
#[inline(always)]
pub fn __read_serial(_buf: &mut [u8]) -> usize {
    0
}

/// Routes the `log` crate's records to Ferric's console. Call it once at start-up.
///
/// Without the link (this release) it does nothing, and logging behaves as if no logger were set.
#[inline(always)]
pub fn init_log() {}
