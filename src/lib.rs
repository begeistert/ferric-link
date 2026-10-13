//! # ferric-link
//!
//! The firmware side of **Ferric Link**, the console that Ferric (a Rust IDE for iPad) opens to a Raspberry Pi
//! Pico over USB, as an ordinary Rust dependency: a project that uses it builds with plain `cargo build` anywhere,
//! and behaves the same inside Ferric and outside it.
//!
//! The board presents itself as a USB Ethernet adapter (CDC-NCM; iPadOS mounts one by itself, while it offers
//! apps no serial port at all). The Link is a DHCP server and a TCP console at `10.55.0.1:1234` that speaks a tiny
//! framing ([`protocol`]). Firmware gets three things:
//!
//! - [`read_serial!`]: the bytes the user typed in Ferric's Serial tab (never blocks);
//! - [`init_log`]: routes the `log` crate there;
//! - `start` (or `start_named`): brings the Link up. It is the only call that differs between flavours.
//!
//! # Features
//!
//! Enable **at most one** flavour; each is the crate's whole USB/network stack on that HAL:
//!
//! | feature   | chip            | built on                                                    | start                         |
//! |-----------|-----------------|-------------------------------------------------------------|-------------------------------|
//! | `rp2040`  | RP2040 (Pico)   | `rp2040-hal`, `usb-device`, `smoltcp`, runs in the USB IRQ  | `start()`                     |
//! | `rp235x`  | RP2350 (Pico 2) | `rp235x-hal`, `usb-device`, `smoltcp`, runs in the USB IRQ  | `start()`                     |
//! | `embassy` | RP2350 (Pico 2) | `embassy-rp`, `embassy-usb`, `embassy-net`, async tasks     | `start(spawner)`              |
//!
//! Others: `log` (default; makes [`init_log`] do something), `always` (see below) and `serial` (also present a
//! CDC-ACM serial port, which the Link Ferric injects never had).
//!
//! # Debug and release
//!
//! The Link is active in **debug builds** (the `dev` profile, i.e. when the crate is compiled with
//! `debug_assertions`) and a stub in release builds, exactly as Ferric always injected it: a release firmware does
//! not carry a USB stack nobody is listening to. The `always` feature keeps it in release too. The rule is decided
//! by this crate's build script, so it is the same inside and outside Ferric. In the stub, `start` returns at once
//! without touching the hardware (your own clock/USB setup runs as if the crate were absent), [`read_serial!`] reads
//! nothing and [`init_log`] does nothing.
//!
//! With **no** HAL feature the same API compiles as stubs on any target, so a `cargo check` on the host works.
//!
//! # Example
//!
//! ```
//! let mut buf = [0u8; 64];
//! let n = ferric_link::read_serial!(&mut buf);
//! assert_eq!(n, 0); // no Link in this build (no HAL feature)
//! ferric_link::init_log();
//! ```
//!
//! See `README.md` for one `main` per flavour, and the `examples/` folder for complete programs.
#![cfg_attr(not(test), no_std)]
#![warn(missing_docs)]

#[cfg(any(
    all(feature = "rp2040", feature = "rp2040-hal-0-10"),
    all(ferric_link_rp2040, feature = "rp235x"),
    all(ferric_link_rp2040, feature = "embassy"),
    all(feature = "rp235x", feature = "embassy")
))]
compile_error!(
    "ferric-link: enable at most one of the `rp2040`, `rp2040-hal-0-10`, `rp235x` and `embassy` features (they are mutually exclusive)"
);

pub mod protocol;

mod identity;
// Internal helpers: `ring` backs the `rp2040`/`rp235x` flavour and `mux` its `serial` option;
// whatever a given feature set leaves unused is still unit-tested on the host.
#[allow(dead_code)]
mod mux;
#[allow(dead_code)]
mod ring;

/// The USB product string; the Link the Ferric app used to inject says this (the host lists it).
#[cfg(all(ferric_link_active, not(feature = "serial")))]
const PRODUCT: &str = "Ferric Link (red sobre USB)";
#[cfg(all(ferric_link_active, not(feature = "serial")))]
const SERIAL_NUMBER: &str = "ferric-link";
#[cfg(all(ferric_link_active, feature = "serial"))]
const PRODUCT: &str = "Ferric Console (serial + red sobre USB)";
#[cfg(all(ferric_link_active, feature = "serial"))]
const SERIAL_NUMBER: &str = "ferric-console";

#[cfg(ferric_link_active)]
mod vendor;

#[cfg(all(ferric_link_active, ferric_link_rp))]
mod rp;
#[cfg(all(ferric_link_active, ferric_link_rp))]
pub use rp::{start, start_named, Console};

#[cfg(all(ferric_link_active, feature = "embassy"))]
mod embassy_flavor;
#[cfg(all(ferric_link_active, feature = "embassy"))]
pub use embassy_flavor::{start, start_named, Console};

#[cfg(ferric_link_stub)]
mod stub;
#[cfg(ferric_link_stub)]
pub use stub::*;

#[cfg(all(feature = "log", ferric_link_active))]
mod log_bridge;

/// Reads what the user typed in Ferric's console into a byte buffer and evaluates to the number of bytes read.
///
/// The argument is anything that borrows as `&mut [u8]` (a `&mut [u8; N]`, a `&mut [u8]`). It never blocks.
/// Without an active Link (no HAL feature, or a release build) it reads nothing and evaluates to `0usize`, so a
/// polling loop simply sees no input.
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
#[inline]
pub fn __read_serial(buf: &mut [u8]) -> usize {
    #[cfg(all(ferric_link_active, ferric_link_rp))]
    return rp::read_bytes(buf);
    #[cfg(all(ferric_link_active, feature = "embassy"))]
    return embassy_flavor::try_read_bytes(buf);
    #[cfg(not(ferric_link_active))]
    {
        let _ = buf;
        0
    }
}

/// Routes the `log` crate's records to Ferric's console, at `Info` level. Call it once, after `start`.
///
/// It does something only where the Link is active and the `log` feature is on (the default); everywhere else it is
/// a no-op and logging behaves as if no logger were set. Safe to call more than once. Which records are forwarded:
/// those of the crate named in `start_named`, or every target after a plain `start`.
#[inline]
pub fn init_log() {
    #[cfg(all(feature = "log", ferric_link_active))]
    log_bridge::install();
}
