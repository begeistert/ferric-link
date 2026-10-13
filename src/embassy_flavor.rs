//! The `embassy` flavour of [`crate::Console`]: an `embassy-executor` task per
//! transport, built on `embassy-usb` and `embassy-net`. Unlike the
//! `rp2040`/`rp235x` flavour there is a real executor here, so each transport
//! gets its own async task instead of one interrupt doing everything.
//!
//! Ported from the code Ferric used to synthesize into a project's
//! `src/ferric_link.rs` at compile time (the part that now lives in the
//! `ferric-link` crate); the CDC-NCM class underneath is the patched copy in
//! `crate::vendor`.

use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_net::tcp::TcpSocket;
use embassy_net::StackResources;
use embassy_rp::bind_interrupts;
use embassy_rp::clocks::RoscRng;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_rp::Peri;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::pipe::Pipe;
#[cfg(feature = "serial")]
use embassy_usb::class::cdc_acm::{CdcAcmClass, Receiver, Sender, State as AcmState};
#[cfg(feature = "serial")]
use embassy_usb::driver::EndpointError;
use embassy_usb::{Builder, Config, UsbDevice};

// The CDC-NCM class is the patched copy in `crate::vendor` (embassy-usb 0.6.0's
// own lacks the Apple fix), not `embassy_usb::class::cdc_ncm`.
use crate::vendor::embassy_ncm::embassy_net::{Device, Runner as NcmRunner, State as NetState};
use crate::vendor::embassy_ncm::{CdcNcmClass, State as NcmState};
use static_cell::StaticCell;

use crate::protocol::{FrameKind, BOOTSEL_PAYLOAD};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

type UsbDriver = Driver<'static, USB>;
const MTU: usize = 1514;

/// Output queued for the Link (drained by `link_task`, framed as `0x01`).
static OUT_LINK: Pipe<CriticalSectionRawMutex, 1024> = Pipe::new();
/// Output queued for CDC-ACM (drained by `cdc_task`, sent raw).
#[cfg(feature = "serial")]
static OUT_CDC: Pipe<CriticalSectionRawMutex, 1024> = Pipe::new();
/// Input for the user's program: bytes from the Link (and CDC-ACM, with `serial`) land here.
static IN: Pipe<CriticalSectionRawMutex, 256> = Pipe::new();

/// A handle to the started console, implementing [`embedded_io_async::Read`]
/// and [`embedded_io_async::Write`]. Cheap and freely copyable: all state
/// lives in the statics above, so any number of `Console` values observe the
/// same one console.
#[derive(Clone, Copy)]
pub struct Console(());

impl Console {
    /// Non-blocking read: returns `0` immediately if nothing is buffered yet,
    /// rather than waiting like [`embedded_io_async::Read::read`] does.
    pub fn try_read(&mut self, buf: &mut [u8]) -> usize {
        try_read_bytes(buf)
    }
}

pub(crate) fn try_read_bytes(buf: &mut [u8]) -> usize {
    IN.try_read(buf).unwrap_or(0)
}

impl embedded_io_async::ErrorType for Console {
    type Error = core::convert::Infallible;
}

impl embedded_io_async::Read for Console {
    /// Waits for at least one byte, from either transport.
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        Ok(IN.read(buf).await)
    }
}

impl embedded_io_async::Write for Console {
    /// Never blocks: queues `buf` for both transports (each drops its oldest
    /// buffered bytes rather than stalling the caller if it falls behind).
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        write_bytes(buf);
        Ok(buf.len())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn write_bytes(buf: &[u8]) {
    let _ = OUT_LINK.try_write(buf);
    #[cfg(feature = "serial")]
    let _ = OUT_CDC.try_write(buf);
}

/// Formats `args` and writes it followed by a newline, to both transports.
/// Used by [`crate::init_log`].
#[cfg_attr(not(feature = "log"), allow(dead_code))]
pub(crate) fn log_fmt(args: core::fmt::Arguments) {
    use core::fmt::Write;
    let mut s = heapless::String::<256>::new();
    let _ = write!(&mut s, "{}", args);
    let _ = s.push('\n');
    write_bytes(s.as_bytes());
}

/// Starts the console: spawns a task that brings up the composite USB device
/// (CDC-ACM + the Ferric Link) and every task that services it. Returns a
/// handle right away; the console finishes attaching in the background.
///
/// Call this **first thing** in your `#[embassy_executor::main]`, before your
/// own peripheral setup: internally this steals `Peripherals::USB` rather
/// than taking it, so your own `Peripherals::take()` afterwards still succeeds.
pub fn start(spawner: Spawner) -> Console {
    bring_up(spawner)
}

/// Like [`start`], and also tells the Link what to call this firmware: `name`
/// (conventionally `env!("CARGO_PKG_NAME")` of the calling crate) is what the
/// host shows as the firmware's identity, and [`crate::init_log`] then forwards
/// only the log records whose target is in that crate (as Ferric always did).
pub fn start_named(spawner: Spawner, name: &'static str) -> Console {
    crate::identity::set(name);
    bring_up(spawner)
}

fn bring_up(spawner: Spawner) -> Console {
    let s = spawner;
    spawner.spawn(__auto(s).unwrap());
    Console(())
}

#[embassy_executor::task]
async fn __auto(spawner: Spawner) {
    let p = unsafe { embassy_rp::Peripherals::steal() };
    init(&spawner, p.USB);
}

fn init(spawner: &Spawner, usb: Peri<'static, USB>) {
    let driver = Driver::new(usb, Irqs);

    let mut config = Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("Ferric");
    config.product = Some(crate::PRODUCT);
    config.serial_number = Some(crate::SERIAL_NUMBER);
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 128]> = StaticCell::new();
    let mut builder = Builder::new(
        driver,
        config,
        &mut CONFIG_DESC.init([0; 256])[..],
        &mut BOS_DESC.init([0; 256])[..],
        &mut [], // no MSOS descriptors
        &mut CONTROL_BUF.init([0; 128])[..],
    );

    // What the host believes is its own adapter's MAC, and ours.
    let host_mac = [0x88, 0x88, 0x88, 0x88, 0x88, 0x88];
    let our_mac = [0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc];

    static NCM_STATE: StaticCell<NcmState> = StaticCell::new();
    let ncm_class = CdcNcmClass::new(&mut builder, NCM_STATE.init(NcmState::new()), host_mac, 64);

    #[cfg(feature = "serial")]
    let (acm_sender, acm_receiver) = {
        static ACM_STATE: StaticCell<AcmState> = StaticCell::new();
        CdcAcmClass::new(&mut builder, ACM_STATE.init(AcmState::new()), 64).split()
    };

    let usb_dev = builder.build();
    spawner.spawn(usb_task(usb_dev).unwrap());
    #[cfg(feature = "serial")]
    spawner.spawn(cdc_task(acm_sender, acm_receiver).unwrap());

    static NET_STATE: StaticCell<NetState<MTU, 4, 4>> = StaticCell::new();
    let (ncm_runner, device) =
        ncm_class.into_embassy_net_device::<MTU, 4, 4>(NET_STATE.init(NetState::new()), our_mac);
    spawner.spawn(ncm_task(ncm_runner).unwrap());

    // The Pico's address is fixed; the host receives its own by DHCP (.50-.200).
    let net_config = embassy_net::Config::ipv4_static(embassy_net::StaticConfigV4 {
        address: embassy_net::Ipv4Cidr::new(embassy_net::Ipv4Address::new(10, 55, 0, 1), 24),
        dns_servers: Default::default(),
        gateway: None,
    });
    let mut rng = RoscRng;
    let seed = rng.next_u64();
    static RESOURCES: StaticCell<StackResources<4>> = StaticCell::new();
    let (stack, net_runner) =
        embassy_net::new(device, net_config, RESOURCES.init(StackResources::new()), seed);
    spawner.spawn(net_task(net_runner).unwrap());
    spawner.spawn(dhcp_task(stack).unwrap());
    spawner.spawn(link_task(stack).unwrap());
}

#[embassy_executor::task]
async fn usb_task(mut device: UsbDevice<'static, UsbDriver>) -> ! {
    device.run().await
}

#[embassy_executor::task]
async fn ncm_task(class: NcmRunner<'static, UsbDriver, MTU>) -> ! {
    class.run().await
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, Device<'static, MTU>>) -> ! {
    runner.run().await
}

/// The host asks for an IP by DHCP when the cable is plugged in; we hand it out.
#[embassy_executor::task]
async fn dhcp_task(stack: embassy_net::Stack<'static>) -> ! {
    use core::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use edge_dhcp::io::{self, DEFAULT_SERVER_PORT};
    use edge_dhcp::server::{Server, ServerOptions};
    use edge_nal::UdpBind;
    use edge_nal_embassy::{Udp, UdpBuffers};

    let ip = Ipv4Addr::new(10, 55, 0, 1);
    static BUFFERS: StaticCell<UdpBuffers<1, 1536, 1536, 2>> = StaticCell::new();
    let udp = Udp::new(stack, BUFFERS.init(UdpBuffers::new()));
    let mut socket = match udp
        .bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, DEFAULT_SERVER_PORT)))
        .await
    {
        Ok(s) => s,
        Err(_) => loop {
            core::future::pending::<()>().await
        },
    };
    let mut buf = [0u8; 1500];
    loop {
        let _ = io::server::run(
            &mut Server::<_, 8>::new_with_et(ip),
            &ServerOptions::new(ip, None),
            &mut socket,
            &mut buf,
        )
        .await;
        embassy_time::Timer::after_millis(500).await;
    }
}

/// A frame going out: kind + len (2 bytes little-endian) + payload.
async fn frame(
    socket: &mut TcpSocket<'_>,
    kind: FrameKind,
    payload: &[u8],
) -> Result<(), embassy_net::tcp::Error> {
    use embedded_io_async::Write;
    let head = crate::protocol::encode_header(kind, payload.len() as u16);
    socket.write_all(&head).await?;
    socket.write_all(payload).await?;
    Ok(())
}

fn dispatch_frame(kind: u8, payload: &[u8]) {
    if kind == FrameKind::Input.as_u8() {
        let _ = IN.try_write(payload);
    } else if kind == FrameKind::Control.as_u8() && payload == BOOTSEL_PAYLOAD {
        // The host asked for BOOTSEL without touching the physical button.
        embassy_rp::rom_data::reset_to_usb_boot(0, 0);
    }
}

/// The Link's TCP service: accepts the host, greets it (`0x04`), and
/// multiplexes output (`OUT_LINK` → `0x01`) and input (`0x02` → `IN`) until
/// the connection drops.
#[embassy_executor::task]
async fn link_task(stack: embassy_net::Stack<'static>) -> ! {
    let mut rx_buffer = [0u8; 2048];
    let mut tx_buffer = [0u8; 2048];
    let mut out = [0u8; 512];
    let mut inbuf = [0u8; 512];
    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        if socket.accept(1234).await.is_err() {
            continue;
        }
        if frame(&mut socket, FrameKind::Hello, crate::identity::name().as_bytes())
            .await
            .is_err()
        {
            continue;
        }
        // Incremental parser for the framing (bytes can arrive split across reads).
        let mut parser: crate::protocol::FrameParser<512> = crate::protocol::FrameParser::new();
        'connection: loop {
            match select(OUT_LINK.read(&mut out), socket.read(&mut inbuf)).await {
                Either::First(n) => {
                    if frame(&mut socket, FrameKind::Output, &out[..n]).await.is_err() {
                        break 'connection;
                    }
                }
                Either::Second(Ok(0)) | Either::Second(Err(_)) => break 'connection,
                Either::Second(Ok(n)) => {
                    for &b in &inbuf[..n] {
                        if let Some(done) = parser.feed(b) {
                            dispatch_frame(done.kind, parser.payload());
                        }
                    }
                }
            }
        }
        socket.abort();
        let _ = socket.flush().await;
    }
}

/// The CDC-ACM service: a plain byte pipe with no framing (a real terminal on
/// the other end just sees a serial port). Waits for the host to open the
/// port, then multiplexes `OUT_CDC` and incoming packets until it disconnects.
#[cfg(feature = "serial")]
#[embassy_executor::task]
async fn cdc_task(mut sender: Sender<'static, UsbDriver>, mut receiver: Receiver<'static, UsbDriver>) -> ! {
    let mut out = [0u8; 512];
    let mut inbuf = [0u8; 64];
    loop {
        receiver.wait_connection().await;
        loop {
            match select(OUT_CDC.read(&mut out), receiver.read_packet(&mut inbuf)).await {
                Either::First(n) => {
                    if write_packet_chunked(&mut sender, &out[..n]).await.is_err() {
                        break;
                    }
                }
                Either::Second(Ok(n)) => {
                    let _ = IN.try_write(&inbuf[..n]);
                }
                Either::Second(Err(_)) => break, // host closed the port
            }
        }
    }
}

#[cfg(feature = "serial")]
/// `write_packet` sends at most one USB packet; a line longer than that (a
/// `log::info!` easily is) needs splitting across several.
async fn write_packet_chunked(
    sender: &mut Sender<'static, UsbDriver>,
    data: &[u8],
) -> Result<(), EndpointError> {
    for chunk in data.chunks(64) {
        sender.write_packet(chunk).await?;
    }
    Ok(())
}
