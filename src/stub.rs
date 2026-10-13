//! The API of the active Link with nothing behind it: what a build gets when a
//! HAL feature is enabled but the Link is not active (a release build without
//! the `always` feature). Same names and signatures, so firmware compiles the
//! same in both profiles; nothing touches the hardware, `start` returns at
//! once, reads find nothing and writes are discarded.

#[cfg(ferric_link_rp)]
mod rp {
    /// A handle to the console. In this build there is no Link: it reads nothing and discards what is written.
    #[derive(Clone, Copy)]
    pub struct Console(());

    /// Starts nothing (the Link is not active in this build) and returns the inert handle.
    pub fn start() -> Console {
        Console(())
    }

    /// Starts nothing (the Link is not active in this build) and returns the inert handle.
    pub fn start_named(_name: &'static str) -> Console {
        Console(())
    }

    impl Console {
        /// Non-blocking read: always `0` in this build.
        pub fn try_read(&mut self, _buf: &mut [u8]) -> usize {
            0
        }
    }

    impl embedded_io::ErrorType for Console {
        type Error = core::convert::Infallible;
    }

    impl embedded_io::Read for Console {
        fn read(&mut self, _buf: &mut [u8]) -> Result<usize, Self::Error> {
            Ok(0)
        }
    }

    impl embedded_io::Write for Console {
        fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
            Ok(buf.len())
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }
}
#[cfg(ferric_link_rp)]
pub use rp::*;

#[cfg(feature = "embassy")]
mod embassy {
    use embassy_executor::Spawner;

    /// A handle to the console. In this build there is no Link: it reads nothing and discards what is written.
    #[derive(Clone, Copy)]
    pub struct Console(());

    /// Starts nothing (the Link is not active in this build) and returns the inert handle.
    pub fn start(_spawner: Spawner) -> Console {
        Console(())
    }

    /// Starts nothing (the Link is not active in this build) and returns the inert handle.
    pub fn start_named(_spawner: Spawner, _name: &'static str) -> Console {
        Console(())
    }

    impl Console {
        /// Non-blocking read: always `0` in this build.
        pub fn try_read(&mut self, _buf: &mut [u8]) -> usize {
            0
        }
    }

    impl embedded_io_async::ErrorType for Console {
        type Error = core::convert::Infallible;
    }

    impl embedded_io_async::Read for Console {
        /// There is never input in this build, so this never completes (as a real console with no host attached).
        async fn read(&mut self, _buf: &mut [u8]) -> Result<usize, Self::Error> {
            core::future::pending().await
        }
    }

    impl embedded_io_async::Write for Console {
        async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
            Ok(buf.len())
        }

        async fn flush(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }
}
#[cfg(feature = "embassy")]
pub use embassy::*;
