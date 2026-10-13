# Linker support for `../examples/`

`memory.x` for each chip, copied from the official `rp2040-project-template` / `rp235x-project-template` (see the
header of each file). `../build.rs` embeds the right one into `OUT_DIR` and adds it to the link search path, but
**only** when `FERRIC_LINK_BUILD_EXAMPLES=1` is set: a library must never hand a downstream consumer's final link a
competing linker script, so providing a `memory.x` is opt-in.

The extra `--features` below only supply what a final binary needs and a real project already gets from its own
dependencies: the HAL's critical-section implementation (and, for embassy, the executor's architecture).

```sh
FERRIC_LINK_BUILD_EXAMPLES=1 cargo build --example rp2040_blink_console \
    --features rp2040,rp2040-hal/critical-section-impl --target thumbv6m-none-eabi

FERRIC_LINK_BUILD_EXAMPLES=1 cargo build --example rp235x_blink_console \
    --features rp235x,rp235x-hal/critical-section-impl --target thumbv8m.main-none-eabihf

FERRIC_LINK_BUILD_EXAMPLES=1 cargo build --example embassy_blink_console \
    --features embassy,embassy-rp/critical-section-impl,embassy-executor/platform-cortex-m,embassy-executor/executor-thread \
    --target thumbv8m.main-none-eabihf
```

These are debug builds, where the Link is active. A release build (`--release`) is the stub; add the `always` feature to
keep the Link there.

A real firmware project needs its own `memory.x` and three-line `build.rs`, as every rp-rs or embassy template has.
