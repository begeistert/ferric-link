//! Blinks the Pico 2's on-board LED, logs each transition, and echoes back
//! whatever it reads from the console (what you type in Ferric's Serial tab).
//!
//! Build: `FERRIC_LINK_BUILD_EXAMPLES=1 cargo build --example rp235x_blink_console --features rp235x,rp235x-hal/critical-section-impl --target thumbv8m.main-none-eabihf`
//! (the `rp235x-hal/critical-section-impl` feature only supplies the critical-section implementation a final
//! binary needs; a real project already gets it from its own HAL dependency).

#![no_std]
#![no_main]

use embedded_hal::digital::OutputPin;
use embedded_io::{Read, Write};
use panic_halt as _;
use rp235x_hal::clocks::init_clocks_and_plls;
use rp235x_hal::{self as hal, entry};
use rp235x_hal::{pac, Clock};

/// Tells the Boot ROM this is a plain, secure executable image.
#[unsafe(link_section = ".start_block")]
#[used]
pub static IMAGE_DEF: hal::block::ImageDef = hal::block::ImageDef::secure_exe();

#[entry]
fn main() -> ! {
    // `ferric_link::start()` steals the peripherals it needs, so it must
    // run before `Peripherals::take()` below — not after.
    let mut console = ferric_link::start();
    ferric_link::init_log();

    let mut pac = pac::Peripherals::take().unwrap();
    let core = cortex_m::Peripherals::take().unwrap();
    let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);
    let sio = hal::Sio::new(pac.SIO);

    // The on-board crystal on every Pico 2 board is 12 MHz. Calling this
    // again after `ferric_link::start()` already did is safe: rp-hal
    // detaches clk_sys/clk_ref from the PLLs before touching them.
    let clocks = init_clocks_and_plls(
        12_000_000,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .ok()
    .unwrap();

    let mut delay = cortex_m::delay::Delay::new(core.SYST, clocks.system_clock.freq().to_Hz());
    let pins = hal::gpio::Pins::new(pac.IO_BANK0, pac.PADS_BANK0, sio.gpio_bank0, &mut pac.RESETS);
    // GP25 on the Pico 2; move this if your board wires the LED elsewhere.
    let mut led = pins.gpio25.into_push_pull_output();

    let mut buf = [0u8; 64];
    loop {
        log::info!("on!");
        led.set_high().unwrap();
        delay.delay_ms(250);

        // Non-blocking: `read` returns 0 immediately when nothing is waiting,
        // from the Link.
        let n = console.read(&mut buf).unwrap();
        if n > 0 {
            let _ = console.write(&buf[..n]);
        }

        log::info!("off!");
        led.set_low().unwrap();
        delay.delay_ms(250);
    }
}
