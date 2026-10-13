//! Blinks the Pico's on-board LED, logs each transition, and echoes back
//! whatever it reads from the console (what you type in Ferric's Serial tab).
//!
//! Build: `FERRIC_LINK_BUILD_EXAMPLES=1 cargo build --example rp2040_blink_console --features rp2040,rp2040-hal/critical-section-impl --target thumbv6m-none-eabi`
//! (the `rp2040-hal/critical-section-impl` feature only supplies the critical-section implementation a final
//! binary needs; a real project already gets it from its own HAL dependency).

#![no_std]
#![no_main]

use cortex_m_rt::entry;
use embedded_hal::digital::OutputPin;
use embedded_io::{Read, Write};
use hal::clocks::{init_clocks_and_plls, Clock};
use hal::{pac, sio::Sio, watchdog::Watchdog};
use panic_halt as _;
use rp2040_hal as hal;

/// The RP2040 boots from this second-stage bootloader; without it the board
/// never reaches `main`.
#[unsafe(link_section = ".boot2")]
#[used]
pub static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

#[entry]
fn main() -> ! {
    // `ferric_link::start()` steals the peripherals it needs, so it must
    // run before `Peripherals::take()` below — not after.
    let mut console = ferric_link::start();
    ferric_link::init_log();

    let mut pac = pac::Peripherals::take().unwrap();
    let core = pac::CorePeripherals::take().unwrap();
    let mut watchdog = Watchdog::new(pac.WATCHDOG);
    let sio = Sio::new(pac.SIO);

    // The on-board crystal on every Pico board is 12 MHz. Calling this again
    // after `ferric_link::start()` already did is safe: rp-hal detaches
    // clk_sys/clk_ref from the PLLs before touching them.
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
    // GP25 on the original Pico; move this if your board wires the LED elsewhere.
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
