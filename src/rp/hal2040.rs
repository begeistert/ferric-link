//! RP2040-specific glue: the handful of things that differ from RP2350 (PAC
//! register names, and how to reboot into BOOTSEL). Everything else in
//! [`super`] is shared between the two chips.

#[cfg(feature = "rp2040")]
pub(crate) use rp2040_hal as hal;
// the same HAL one release back, which is what `rp-pico` 0.9 depends on
#[cfg(feature = "rp2040-hal-0-10")]
pub(crate) use rp2040_hal_0_10 as hal;

pub(crate) type Bus = hal::usb::UsbBus;

pub(crate) fn usb_clock() -> Option<hal::clocks::UsbClock> {
    let mut resets = unsafe { hal::pac::RESETS::steal() };
    let mut wd = hal::watchdog::Watchdog::new(unsafe { hal::pac::WATCHDOG::steal() });
    hal::clocks::init_clocks_and_plls(
        12_000_000,
        unsafe { hal::pac::XOSC::steal() },
        unsafe { hal::pac::CLOCKS::steal() },
        unsafe { hal::pac::PLL_SYS::steal() },
        unsafe { hal::pac::PLL_USB::steal() },
        &mut resets,
        &mut wd,
    )
    .ok()
    .map(|c| c.usb_clock)
}

pub(crate) fn usb_bus(clk: hal::clocks::UsbClock) -> Bus {
    let mut resets = unsafe { hal::pac::RESETS::steal() };
    hal::usb::UsbBus::new(
        unsafe { hal::pac::USBCTRL_REGS::steal() },
        unsafe { hal::pac::USBCTRL_DPRAM::steal() },
        clk,
        true,
        &mut resets,
    )
}

pub(crate) fn sof_enable() {
    unsafe { hal::pac::USBCTRL_REGS::steal() }
        .inte()
        .modify(|_, w| w.dev_sof().set_bit());
}

/// Did a start-of-frame arrive? (and clears it by reading SOF_RD). This is the
/// Link's 1 kHz clock: the host sends one every millisecond regardless of
/// what the user's program does with its own timers.
pub(crate) fn sof_tick() -> bool {
    let usb = unsafe { hal::pac::USBCTRL_REGS::steal() };
    let sof = usb.ints().read().dev_sof().bit_is_set();
    let _ = usb.sof_rd().read().bits();
    sof
}

pub(crate) fn irq_enable() {
    unsafe { cortex_m::peripheral::NVIC::unmask(hal::pac::Interrupt::USBCTRL_IRQ) };
}

pub(crate) fn pend() {
    cortex_m::peripheral::NVIC::pend(hal::pac::Interrupt::USBCTRL_IRQ);
}

pub(crate) fn bootsel() -> ! {
    hal::rom_data::reset_to_usb_boot(0, 0);
    loop {
        cortex_m::asm::wfi();
    }
}
