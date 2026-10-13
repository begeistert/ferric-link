# ferric-link

The firmware side of **Ferric Link**, the console that Ferric (a Rust IDE for iPad) opens to a Raspberry Pi Pico
over USB.

Firmware written in Ferric uses `read_serial!` to read what you type in the IDE's console and `init_log()` to send
`log` records there. With this crate as a dependency, the same project builds with plain `cargo build` on any machine.

```toml
[dependencies]
ferric-link = "0.1"
```

```rust
let mut buf = [0u8; 64];
let n = ferric_link::read_serial!(&mut buf);   // bytes read; never blocks
ferric_link::init_log();
```

## Status

0.1 is the no-op API: `read_serial!` reads nothing (it returns `0`) and `init_log()` does nothing. The USB link
itself (CDC-NCM with a TCP console) comes in a later release behind a `link` feature, with the same calls, so code
written against 0.1 keeps compiling.

`#![no_std]`, no dependencies, no `unsafe`.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this crate by you,
as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
