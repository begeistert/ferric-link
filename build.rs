//! Two jobs.
//!
//! 1. Decide, once, whether the Link is *active* in this build, and say so to
//!    the compiler as a cfg. The rule is the same wherever the crate is built
//!    (inside Ferric or with a plain `cargo build`): the Link is active when a
//!    HAL feature (`rp2040`, `rp2040-hal-0-10`, `rp235x`, `embassy`) is enabled **and** either
//!    debug assertions are on for this crate (the default `dev` profile) or the
//!    `always` feature is enabled. A HAL feature without that is the *stub*
//!    build (same API, nothing behind it).
//!
//!    ```text
//!    ferric_link_rp       the rp2040 / rp2040-hal-0-10 / rp235x flavour
//!    ferric_link_rp2040   the RP2040 one of those
//!    ferric_link_active   the real Link
//!    ferric_link_stub     the API as stubs, with a HAL feature
//!    (neither)            no HAL feature: the 0.1 stubs
//!    ```
//!
//! 2. Pick the right `memory.x` for **this crate's own examples** and put it
//!    where `cortex-m-rt`'s linker script (`link.x`, pulled in by
//!    `.cargo/config.toml`) can find it. A library must never hand a
//!    `memory.x`/link-search path to a downstream consumer's final link (memory
//!    layout is the application's call), so this only does anything when
//!    `FERRIC_LINK_BUILD_EXAMPLES=1` is set, which only our own example builds
//!    do. See `examples-support/README.md`.

use std::env;
use std::fs;
use std::path::PathBuf;

fn on(var: &str) -> bool {
    env::var_os(var).is_some()
}

fn main() {
    println!("cargo:rustc-check-cfg=cfg(ferric_link_active)");
    println!("cargo:rustc-check-cfg=cfg(ferric_link_stub)");
    println!("cargo:rustc-check-cfg=cfg(ferric_link_rp)");
    println!("cargo:rustc-check-cfg=cfg(ferric_link_rp2040)");
    let rp2040 = on("CARGO_FEATURE_RP2040") || on("CARGO_FEATURE_RP2040_HAL_0_10");
    let rp = rp2040 || on("CARGO_FEATURE_RP235X");
    let hal = rp || on("CARGO_FEATURE_EMBASSY");
    if rp2040 {
        println!("cargo:rustc-cfg=ferric_link_rp2040");
    }
    if rp {
        // the synchronous flavour (usb-device + smoltcp), on either chip
        println!("cargo:rustc-cfg=ferric_link_rp");
    }
    // Set by cargo for build scripts exactly when the crate itself is compiled with `debug_assertions`.
    let debug = on("CARGO_CFG_DEBUG_ASSERTIONS");
    if hal {
        if debug || on("CARGO_FEATURE_ALWAYS") {
            println!("cargo:rustc-cfg=ferric_link_active");
        } else {
            println!("cargo:rustc-cfg=ferric_link_stub");
        }
    }

    println!("cargo:rerun-if-changed=build.rs");
    // Must run before the early return below, or cargo never learns to watch
    // this variable and a later build with it newly set stays cached stale.
    println!("cargo:rerun-if-env-changed=FERRIC_LINK_BUILD_EXAMPLES");
    if !on("FERRIC_LINK_BUILD_EXAMPLES") {
        return;
    }

    let target = env::var("TARGET").unwrap_or_default();
    let memory_x: &str = match target.as_str() {
        "thumbv6m-none-eabi" => include_str!("examples-support/rp2040/memory.x"),
        // Both the `rp235x` and the `embassy` flavours build here.
        "thumbv8m.main-none-eabihf" => include_str!("examples-support/rp235x/memory.x"),
        _ => return, // host target: nothing to link
    };

    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("memory.x"), memory_x).expect("failed to write memory.x to OUT_DIR");
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=examples-support/rp2040/memory.x");
    println!("cargo:rerun-if-changed=examples-support/rp235x/memory.x");
}
