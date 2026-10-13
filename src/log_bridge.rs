//! Wires the `log` crate to the console, so `log::info!` works with no extra
//! plumbing. Available with the default `log` feature, in builds where the
//! Link is active.
//!
//! The Link Ferric used to inject lived inside the user's own crate and
//! filtered by `env!("CARGO_CRATE_NAME")`. As a separate library this crate
//! cannot know that name by itself: `start_named(.., env!("CARGO_PKG_NAME"))`
//! tells it, and then only that crate's records are forwarded. After a plain
//! `start()` every target is forwarded.

struct ConsoleLogger;

impl log::Log for ConsoleLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        crate::identity::log_enabled(metadata.target())
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            log_line(*record.args());
        }
    }

    fn flush(&self) {}
}

fn log_line(args: core::fmt::Arguments) {
    #[cfg(ferric_link_rp)]
    crate::rp::log_fmt(args);
    #[cfg(feature = "embassy")]
    crate::embassy_flavor::log_fmt(args);
}

static LOGGER: ConsoleLogger = ConsoleLogger;

/// Installs the console as the `log` crate's global logger, at
/// [`log::LevelFilter::Info`]. Called by [`crate::init_log`].
pub(crate) fn install() {
    // RP2040's thumbv6m core has no CAS: `log` only offers the `_racy`
    // variants there, which are sound here because we set the logger up
    // before anything else can be logging yet.
    #[cfg(target_has_atomic = "ptr")]
    {
        let _ = log::set_logger(&LOGGER);
        log::set_max_level(log::LevelFilter::Info);
    }
    #[cfg(not(target_has_atomic = "ptr"))]
    unsafe {
        let _ = log::set_logger_racy(&LOGGER);
        log::set_max_level_racy(log::LevelFilter::Info);
    }
}
