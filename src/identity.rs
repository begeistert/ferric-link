//! What the firmware calls itself: the name sent in the Link's hello frame and
//! the crate whose log records are forwarded. The storage exists only where
//! the Link does; the matching rule is pure so it is tested on the host.

/// Whether a `log` target belongs to the crate called `name`. Cargo spells a
/// package name with `-` and a crate name with `_`; both match, so
/// `env!("CARGO_PKG_NAME")` can be passed as is. This is the rule the Link
/// injected by Ferric always had (`target.starts_with(CARGO_CRATE_NAME)`).
#[cfg_attr(not(all(ferric_link_active, feature = "log")), allow(dead_code))]
pub(crate) fn target_in_crate(target: &str, name: &str) -> bool {
    let (t, n) = (target.as_bytes(), name.as_bytes());
    t.len() >= n.len()
        && t.iter().zip(n).all(|(&t, &n)| {
            let fold = |b: u8| if b == b'-' { b'_' } else { b };
            fold(t) == fold(n)
        })
}

#[cfg(ferric_link_active)]
mod state {
    use core::cell::Cell;
    use critical_section::Mutex;

    /// The crate whose logs are forwarded; `None` forwards every target.
    static FILTER: Mutex<Cell<Option<&'static str>>> = Mutex::new(Cell::new(None));

    /// Sets the firmware's name (see `start_named`).
    pub(crate) fn set(name: &'static str) {
        critical_section::with(|cs| FILTER.borrow(cs).set(Some(name)));
    }

    /// The name for the hello frame: the one set by `start_named`, else this crate's own.
    pub(crate) fn name() -> &'static str {
        critical_section::with(|cs| FILTER.borrow(cs).get()).unwrap_or(env!("CARGO_PKG_NAME"))
    }

    /// Whether a log record with this target is forwarded to the console.
    #[cfg(feature = "log")]
    pub(crate) fn log_enabled(target: &str) -> bool {
        match critical_section::with(|cs| FILTER.borrow(cs).get()) {
            Some(name) => super::target_in_crate(target, name),
            None => true,
        }
    }
}
#[cfg(ferric_link_active)]
pub(crate) use state::{name, set};
#[cfg(all(ferric_link_active, feature = "log"))]
pub(crate) use state::log_enabled;

#[cfg(test)]
mod tests {
    use super::target_in_crate;

    #[test]
    fn a_target_matches_its_crate_and_its_modules() {
        assert!(target_in_crate("my_app", "my_app"));
        assert!(target_in_crate("my_app::sensors", "my_app"));
        assert!(!target_in_crate("embassy_rp", "my_app"));
        assert!(!target_in_crate("my", "my_app"));
    }

    #[test]
    fn a_package_name_with_dashes_matches_the_crate_name() {
        assert!(target_in_crate("pico2_link", "pico2-link"));
        assert!(target_in_crate("pico2_link::main", "pico2-link"));
        assert!(!target_in_crate("pico2_lin", "pico2-link"));
    }
}
