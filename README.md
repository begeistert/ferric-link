# ferric-link

The firmware side of **Ferric Link**, the console that [Ferric](https://getferric.dev) (a Rust IDE for iPad) opens to a
Raspberry Pi Pico or Pico 2 over USB.

iPadOS gives apps no API for a USB serial port, but it mounts a standard USB Ethernet adapter on its own. So the board
presents itself as one (CDC-NCM), serves DHCP, and listens for a TCP console at `10.55.0.1:1234`; Ferric's Serial tab
talks to it. This crate is that firmware, as an ordinary dependency: a project that uses it builds with plain
`cargo build` anywhere, and runs exactly the same inside Ferric and outside it. You also get:

- `read_serial!(buf)`: the bytes you typed in the Serial tab (never blocks, returns the count);
- `init_log()`: routes the `log` crate to the Serial tab;
- a "reboot to BOOTSEL" control, so Ferric can flash the board again without touching the button.

```toml
[dependencies]
ferric-link = { version = "0.2", features = ["rp235x"] }   # or "rp2040", "rp2040-hal-0-10", "embassy"
```

## Features

Pick **one** flavour. Each brings the whole USB/network stack for that HAL, so the project does not list `usb-device`,
`smoltcp`, `usbd-ethernet` or any patch itself.

| Feature           | Chip            | Built on                                                       | Start                    |
|-------------------|-----------------|----------------------------------------------------------------|--------------------------|
| `rp2040`          | RP2040 (Pico)   | `rp2040-hal` 0.11, `usb-device`, `smoltcp` (runs in the USB interrupt) | `ferric_link::start()`   |
| `rp2040-hal-0-10` | RP2040 (Pico)   | the same on `rp2040-hal` 0.10, which `rp-pico` 0.9 uses       | `ferric_link::start()`   |
| `rp235x`          | RP2350 (Pico 2) | `rp235x-hal` 0.4, `usb-device`, `smoltcp` (runs in the USB interrupt) | `ferric_link::start()`   |
| `embassy`         | RP2350 (Pico 2) | `embassy-rp` 0.10, `embassy-usb` 0.6, `embassy-net` 0.9 (async tasks) | `ferric_link::start(spawner)` |

The Link has to use the **same copy of the HAL** as your project (two copies fail to link), so pick `rp2040-hal-0-10`
when your project depends on `rp-pico` 0.9, and `rp2040` when it depends on `rp2040-hal` 0.11.

Other features:

- `log` (default): makes `init_log()` do something.
- `always`: keep the Link in release builds (see below).
- `serial`: also present a CDC-ACM serial port (`/dev/tty.usbmodem…`) next to the Link. Off by default: the Link Ferric
  has always injected has none, and a plain serial port is no use on an iPad.

### Debug and release

The Link is **active in debug builds** (when this crate is compiled with `debug_assertions`, i.e. the default `dev`
profile) and a **stub in release builds**, as Ferric has always done it: a release firmware does not carry a USB stack
nobody is listening to, and it does not take over the USB peripheral. Enable `always` to keep it in release.

In the stub the API is identical: `start()` returns at once without touching the hardware, `read_serial!` reads nothing,
`init_log()` does nothing. The rule is decided by this crate's build script, so it is the same whether you build in
Ferric or on your computer. If you set `[profile.dev] debug-assertions = false`, the Link is a stub there too.

With **no** HAL feature the same API (`read_serial!`, `init_log`) compiles as stubs on any target, so `cargo check`
on your computer works for a project that depends on it.

## Using it

Call `start` **first thing** in `main`, before touching other peripherals: it steals what it needs (the clocks, the USB
block), so your own `Peripherals::take()` and `init_clocks_and_plls` afterwards still work. `start_named` also tells the
Link your firmware's name (shown by Ferric as "firmware: …"), and makes `init_log()` forward only your own crate's
records, which is what Ferric always did.

### `rp235x` / `rp2040` / `rp2040-hal-0-10`

```rust
#[entry]
fn main() -> ! {
    ferric_link::start_named(env!("CARGO_PKG_NAME"));
    ferric_link::init_log();

    let mut pac = pac::Peripherals::take().unwrap();
    // ... your own clocks and pins, as usual ...

    let mut buf = [0u8; 64];
    loop {
        log::info!("tick");
        let n = ferric_link::read_serial!(&mut buf); // 0 when nothing was typed
        // ...
    }
}
```

`start()` returns a `Console` implementing `embedded_io::{Read, Write}` (0.6) and `try_read`, if you prefer a handle to the
macro; writing to it is the same as `log::info!` (the Serial tab).

### `embassy`

```rust
#[embassy_executor::main]
async fn main(spawner: Spawner) {
    ferric_link::start_named(spawner, env!("CARGO_PKG_NAME"));
    ferric_link::init_log();

    let p = embassy_rp::init(Default::default());
    // ... your own peripherals ...

    let mut buf = [0u8; 64];
    loop {
        let n = ferric_link::read_serial!(&mut buf);
        // ...
    }
}
```

`start(spawner)` returns a `Console` implementing `embedded_io_async::{Read, Write}` (0.7) and `try_read`. Your project
needs `embassy-executor` with an architecture feature, and `embassy-rp` with `time-driver` and a critical-section
implementation, as any embassy project does.

The `examples/` folder has a complete blink program for each flavour; `examples-support/README.md` says how to build
them.

## Your project's own setup

- **`memory.x` and the link script**: yours, as in any rp-rs or embassy project. This crate does not hand your final
  link anything.
- **critical-section**: the HAL's own implementation (`critical-section-impl` on the HAL), as your project already has.
- **log**: with `log = { version = "0.4", features = ["release_max_level_off"] }` the records disappear from release
  builds, which is what Ferric's templates do.

## About the USB classes

Apple's macOS 26 / iPadOS 26 leave a CDC-NCM data interface disabled when the function declares no network
capabilities (Apple bug report r.170072016). `usbd-ethernet` 0.4.0 and `embassy-usb` 0.6.0 both have that bug, and
the fix is not in a release of either. This crate carries patched copies of the two CDC-NCM classes in `src/vendor/`
(with their licenses), so you do not need `[patch.crates-io]`, a fork or Ferric's build step. See
[`src/vendor/README.md`](src/vendor/README.md).

## Compatibility

`rust-version` is 1.85, enough for the `rp2040`, `rp2040-hal-0-10`, `rp235x` flavours and the stubs. The `embassy`
flavour's dependencies currently need a newer compiler (1.93 at the time of release); Ferric uses a current stable.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
The vendored copies under `src/vendor/` keep their own licenses (MIT for `usbd-ethernet`, MIT OR Apache-2.0 for
`embassy-usb`).

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this crate by you,
as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
