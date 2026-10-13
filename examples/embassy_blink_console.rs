//! Blinks the Pico 2's on-board LED, logs each transition, and echoes back
//! whatever it reads from the console (what you type in Ferric's Serial tab). The `embassy` flavour targets
//! RP2350 only today, matching Ferric's own `embassy` project template.
//!
//! Build: `FERRIC_LINK_BUILD_EXAMPLES=1 cargo build --example embassy_blink_console --features embassy,embassy-rp/critical-section-impl,embassy-executor/platform-cortex-m,embassy-executor/executor-thread --target thumbv8m.main-none-eabihf`
//! (the `embassy-rp/critical-section-impl` and `embassy-executor/*` features only supply what a final
//! binary needs; a real project already gets them from its own dependencies).

#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_time::Timer;
use embedded_io_async::Write;
use panic_halt as _;

/// Tells the Boot ROM this is a plain, secure executable image.
#[unsafe(link_section = ".start_block")]
#[used]
pub static IMAGE_DEF: embassy_rp::block::ImageDef = embassy_rp::block::ImageDef::secure_exe();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // Must run before `embassy_rp::init` below: it steals the USB peripheral
    // rather than taking it, so the real `init` afterwards still succeeds.
    let mut console = ferric_link::start(spawner);
    ferric_link::init_log();

    let p = embassy_rp::init(Default::default());
    // GP25 on the Pico 2; move this if your board wires the LED elsewhere.
    let mut led = Output::new(p.PIN_25, Level::Low);

    let mut buf = [0u8; 64];
    loop {
        log::info!("on!");
        led.set_high();
        Timer::after_millis(250).await;

        // Non-blocking: returns 0 immediately when nothing is waiting yet.
        let n = console.try_read(&mut buf);
        if n > 0 {
            let _ = console.write(&buf[..n]).await;
        }

        log::info!("off!");
        led.set_low();
        Timer::after_millis(250).await;
    }
}
