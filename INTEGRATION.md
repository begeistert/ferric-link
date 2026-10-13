# Adopting ferric-link in Ferric's templates

For Ferric's maintainers: what a project that depends on `ferric-link` 0.2 looks like, and how it differs from what
the app injects into `src/main.rs` and `src/ferric_link.rs` at build time today. The rule is that the crate behaves
the same inside Ferric and outside it, so there is nothing to inject: what `Cargo.toml` and `main.rs` declare is what
runs.

## The dependency line

Pick the feature from the HAL the project already uses (the decision `RpHead` makes today):

| Project depends on                         | Line in `[dependencies]`                                                  |
|--------------------------------------------|---------------------------------------------------------------------------|
| `rp235x-hal` (0.4)                         | `ferric-link = { version = "0.2", features = ["rp235x"] }`                |
| `rp-pico` 0.9 (it brings `rp2040-hal` 0.10) | `ferric-link = { version = "0.2", features = ["rp2040-hal-0-10"] }`       |
| `rp2040-hal` 0.11                          | `ferric-link = { version = "0.2", features = ["rp2040"] }`                |
| `embassy-rp` (RP2350, `rp235xa`)           | `ferric-link = { version = "0.2", features = ["embassy"] }`               |

The Link and the project must share **one** copy of the HAL; that is why the RP2040 has two features. Nothing else is
needed: not `usb-device`, `usbd-ethernet`, `smoltcp`, `critical-section` (`WithLinkDeps` goes away), no `[patch]`
section and no `use defmt_rtt as _;` (the vendored USB class no longer forces `usb-device`'s `defmt` feature).
`log` stays in the project, as today (`features = ["release_max_level_off"]`).

## The calls in `main`

```rust
// rp-rs (rp235x-hal, rp-pico, rp2040-hal): today `ferric_link::auto();` inside `#[cfg(debug_assertions)]`
#[entry]
fn main() -> ! {
    ferric_link::start_named(env!("CARGO_PKG_NAME"));
    ferric_link::init_log();          // only if the project has `log`
    /* the user's main */
}

// embassy: today `ferric_link::auto(&spawner);`
#[embassy_executor::main]
async fn main(spawner: Spawner) {
    ferric_link::start_named(spawner, env!("CARGO_PKG_NAME"));
    ferric_link::init_log();
    /* the user's main */
}
```

- No `#[cfg(debug_assertions)]` and no `mod ferric_link;`: in a release build `start_named` is a stub that returns at
  once, and `read_serial!` evaluates to `0usize`. Both are the same in Ferric and in `cargo build`.
- Both calls fit on the line of the `fn main` (or `async fn main`), so they stay line-neutral.
- `read_serial!` is exported by the crate: `use ferric_link::read_serial;` once (the template's `use` list) lets the
  user's code keep calling `read_serial!(&mut buf)`. It takes anything that borrows as `&mut [u8]`, as before.
- Signatures: `start() -> Console`, `start_named(name: &'static str) -> Console` (rp-rs); `start(spawner: Spawner) ->
  Console`, `start_named(spawner: Spawner, name: &'static str) -> Console` (embassy); `read_serial!(buf) -> usize`;
  `init_log()`. `Console` is `Copy`, implements `embedded_io::{Read, Write}` (0.6; `embedded_io_async` 0.7 with
  `embassy`) and has `try_read(&mut self, &mut [u8]) -> usize`.
- Call it before anything else in `main`, as `auto()` was: it steals the clocks and the USB block.

## Debug and release

Active when the crate is compiled with `debug_assertions`, a stub otherwise; the `always` feature keeps it in release.
This is what the app does today, now decided by the crate's `build.rs` from `CARGO_CFG_DEBUG_ASSERTIONS`, so there is a
single rule. The templates already set `debug-assertions` per profile (true in `dev`, false in `release`).

## Differences from what is injected today

What the crate does *not* change (checked against `LinkSources` in `templates.json` and `FerricLink.cs`): the wire
protocol and framing, `10.55.0.1/24`, the TCP console on `:1234`, DHCP from `.50` to `.200` with no router and no DNS,
the hello frame `0x04` on connect, `bootsel` control frame, the MACs (`88:..` host, `cc:..` Pico), VID/PID
`0xc0de:0xcafe`, the manufacturer / product / serial strings, the buffer sizes, the SOF-driven interrupt for rp-rs, the
`Info` log level, `delay_ms` in the user's code not dropping the connection. Differences, from `ferric-console` and
from the injected code:

1. **No CDC-ACM by default.** `ferric-console` always added a serial port next to the Link, and its strings said
   `Ferric Console`. The injected Link has none, so by default neither does 0.2 (and the strings are the injected ones:
   `Ferric Link (red sobre USB)`, `ferric-link`). The `serial` feature restores it.
2. **The hello name.** The injected code sent `env!("CARGO_PKG_NAME")` of the *user's* crate (Ferric shows
   "firmware: <name>"). A library only sees its own, so `start_named(env!("CARGO_PKG_NAME"))` passes the user's; a plain
   `start()` says `ferric-link`.
3. **The log filter.** The injected logger forwarded only targets starting with `CARGO_CRATE_NAME`. `ferric-console`
   forwarded everything. `start_named` restores the injected filter (the name's `-` counts as `_`); after a plain
   `start()` every target is forwarded.
4. **Names.** `auto()` / `auto(&spawner)` are `start()` / `start(spawner)`; `try_read` became `read_serial!`;
   `log_fmt`, `read().await` and `init(spawner, usb)` of the embassy module are gone from the public API (the
   `Console` handle has async `Read`/`Write`).
5. **The USB classes are vendored.** rp-rs: `usbd-ethernet` 0.4.0, patched, no `defmt`. embassy: the patched CDC-NCM class
   of `embassy-usb` 0.6.0 (the app used to patch `embassy-usb` through `crate-patches.json`; `ferric-console`'s docs said
   the embassy flavour needed no patch, which is wrong for 0.6.0). The `crate-patches.json` entries for `usbd-ethernet`
   and for the NCM change in `embassy-usb` are no longer needed by this crate (other parts of that patch set are for wasm).
6. **Features the crate turns on in the project's HAL:** `rt` on the rp HALs (the `#[interrupt]` handler needs it) and,
   for `embassy`, `embassy-rp`'s `rp235xa` and `time-driver`. It does not turn on `critical-section-impl` or the
   executor's architecture: those stay the project's, as they are in the templates today.
7. **Ownership of `Cargo.toml`.** Nothing is rewritten at build time: the dependency line is part of the project.
8. **Pico 1:** the `rp-pico` template resolves `rp2040-hal` 0.10.2; `rp2040-hal-0-10` is the matching feature.

Not carried over from `ferric-console`: the `probe` feature (the GDB-RSP byte pipe on `:3333`); Ferric's probe
templates keep their own copy under `probe-template/shared/ferric-console`.

## What to test on a Pico 2 (nothing here has run on hardware)

1. Flash `examples/rp235x_blink_console` (debug): `FERRIC_LINK_BUILD_EXAMPLES=1 cargo build --example
   rp235x_blink_console --features rp235x,rp235x-hal/critical-section-impl --target thumbv8m.main-none-eabihf`, then
   convert with `elf2uf2-rs`/`picotool`. Plug it into the iPad: Settings should show an "Ethernet" adapter, and Ferric's
   Serial tab should connect and print `· firmware: ferric-link` and the `on!` / `off!` lines.
2. Type a line in the Serial tab: the example echoes it back.
3. On the Mac instead: `nc 10.55.0.1 1234` (the Mac gets `10.55.0.50`; wait a few seconds for DHCP). You should see raw
   frames (`\x04` + length + `ferric-link`, then `\x01`…). Wi-Fi must stay the default route.
4. Repeat with `embassy_blink_console` (same checks).
5. The "reboot to BOOTSEL" action of the Serial tab should land in RPI-RP2 / RP2350 mode.
6. Release: build with `--release`; the board should run the blink but show no Ethernet adapter. With `--features always`
   it should behave as the debug build.
7. A delay: the examples use 250 ms delays; the connection must survive them (rp-rs flavours run in the USB interrupt).
8. Unplug / replug while Ferric is open: it reconnects within a few seconds.
