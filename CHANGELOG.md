# Changelog

All notable changes to `ferric-link` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this project follows
[Semantic Versioning](https://semver.org/).

## [0.2.0]

The Link itself. 0.1 was the no-op API; code written against it keeps compiling.

### Added
- The Ferric Link: the board presents a USB Ethernet adapter (CDC-NCM), serves DHCP (a pool of `10.55.0.50`..`.200`,
  no router and no DNS, so the host keeps its Wi-Fi) from `10.55.0.1`, and listens for a TCP console on `:1234`. The
  wire protocol is public in `protocol` (frames `0x01` output, `0x02` input, `0x03` control with `bootsel`, `0x04` hello).
- Three flavours, mutually exclusive features: `rp2040` (`rp2040-hal` 0.11) and `rp2040-hal-0-10` (`rp2040-hal` 0.10,
  for `rp-pico` 0.9), `rp235x` (`rp235x-hal` 0.4), and `embassy` (`embassy-rp` 0.10, RP2350 only). The first three run
  inside the USB interrupt on `usb-device` and `smoltcp`; `embassy` runs as tasks on `embassy-usb` and `embassy-net`.
- `start()` (`start(spawner)` with `embassy`) and `start_named(name)` (`start_named(spawner, name)`), returning a
  `Console` that implements `embedded_io::{Read, Write}` (`embedded_io_async` with `embassy`) and has `try_read`.
- `read_serial!` and `init_log()` are now real in an active build (and still `read_serial!(buf) -> usize` and
  `init_log()` everywhere).
- Features `log` (default), `always` and `serial` (CDC-ACM next to the Link, off by default).
- Patched copies of the two CDC-NCM classes in `src/vendor/` (Apple bug r.170072016), so no `[patch.crates-io]` is needed.
- Examples for each flavour; CI builds every flavour for its target, the examples, and the stubs on the host.

### Behaviour
- The Link is **active in debug builds and a stub in release builds**, decided by the crate's own build script
  (`debug_assertions`), so it is the same inside and outside Ferric. The `always` feature keeps it in release.
- With no HAL feature, or with a HAL feature in a release build, the API is the same stubs as 0.1 (`start` returns at
  once without touching the hardware).
- `rust-version` 1.85; edition 2021.
- Differences from the Link the Ferric app injected until now: see `INTEGRATION.md`.

## [0.1.0]

The no-op API: `read_serial!` returns `0`, `init_log()` does nothing.
