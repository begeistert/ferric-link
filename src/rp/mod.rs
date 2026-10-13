//! The `rp2040` / `rp235x` flavour of [`crate::Console`]: a synchronous,
//! interrupt-driven composite USB device built directly on `usb-device` and
//! `smoltcp`. There is no executor here — the whole console (the Link's
//! smoltcp interface, its DHCP server and its TCP service) lives
//! inside the `USBCTRL_IRQ` interrupt, so a `delay_ms` in the user's own
//! `main` never stalls a connection. The interrupt's own clock is the host's
//! start-of-frame (1 kHz), so the console does not claim a `TIMER` either.
//!
//! Ported from the code Ferric used to synthesize into a project's
//! `src/ferric_link.rs` at compile time (the part that now lives in the
//! `ferric-link` crate); the USB class underneath is the patched copy in
//! `crate::vendor`.

use core::cell::UnsafeCell;
use smoltcp::iface::{Config as IfConfig, Interface, SocketHandle, SocketSet, SocketStorage};
use smoltcp::socket::{tcp, udp};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, IpEndpoint, IpListenEndpoint};
use usb_device::bus::UsbBusAllocator;
use usb_device::device::{StringDescriptors, UsbDevice, UsbDeviceBuilder, UsbVidPid};
use crate::vendor::usbd_ethernet::{DeviceState, Ethernet};
#[cfg(feature = "serial")]
use usbd_serial::SerialPort;

#[cfg(feature = "serial")]
use crate::mux::read_either;
use crate::protocol::{FrameKind, FrameParser, BOOTSEL_PAYLOAD};
use crate::ring::Ring;

#[cfg(ferric_link_rp2040)]
#[path = "hal2040.rs"]
mod hal_mod;
#[cfg(feature = "rp235x")]
#[path = "hal235x.rs"]
mod hal_mod;

use hal_mod as hal_impl;
use hal_impl::hal::pac::interrupt;

type Bus = hal_impl::Bus;

/// What the host believes is its own adapter's MAC, and ours. Fixed values
/// (not randomized) so a re-plugged board keeps the same identity the host
/// already learned — this is the Link's protocol, not free to change.
const HOST_MAC: [u8; 6] = [0x88; 6];
const OUR_MAC: [u8; 6] = [0xcc; 6];
/// The Link's fixed address; the host receives its own via DHCP.
const IP: [u8; 4] = [10, 55, 0, 1];
const PORT: u16 = 1234;
/// Socket slots: the console's TCP and the DHCP server's UDP.
const NUM_SOCKS: usize = 2;
const NTB: usize = 2048;
/// DHCP pool, matching the embassy flavour's `edge-dhcp` range (`.50..=.200`).
const POOL_LO: u8 = 50;
const POOL_HI: u8 = 200;

// ── static storage without `static mut` (edition 2024 forbids it) ──────────
struct Shared<T>(UnsafeCell<T>);
unsafe impl<T> Sync for Shared<T> {}
impl<T> Shared<T> {
    const fn new(v: T) -> Self {
        Self(UnsafeCell::new(v))
    }
    #[allow(clippy::mut_from_ref)]
    unsafe fn get(&'static self) -> &'static mut T {
        unsafe { &mut *self.0.get() }
    }
}

/// Zeroed bytes only, so the ~9 KiB of buffers land in `.bss` and cost no flash.
struct Bufs {
    eth_in: [u8; NTB],
    eth_out: [u8; NTB],
    tcp_rx: [u8; 1536],
    tcp_tx: [u8; 1536],
    udp_rx: [u8; 1024],
    udp_tx: [u8; 1024],
}
struct Metas {
    udp_rx: [udp::PacketMetadata; 2],
    udp_tx: [udp::PacketMetadata; 2],
    socks: [SocketStorage<'static>; NUM_SOCKS],
}

static BUFS: Shared<Bufs> = Shared::new(Bufs {
    eth_in: [0; NTB],
    eth_out: [0; NTB],
    tcp_rx: [0; 1536],
    tcp_tx: [0; 1536],
    udp_rx: [0; 1024],
    udp_tx: [0; 1024],
});
static METAS: Shared<Metas> = Shared::new(Metas {
    udp_rx: [udp::PacketMetadata::EMPTY; 2],
    udp_tx: [udp::PacketMetadata::EMPTY; 2],
    socks: [SocketStorage::EMPTY; NUM_SOCKS],
});
static ALLOC: Shared<Option<UsbBusAllocator<Bus>>> = Shared::new(None);
static LINK: Shared<Option<Link>> = Shared::new(None);

struct Link {
    usb: UsbDevice<'static, Bus>,
    eth: Ethernet<'static, Bus>,
    #[cfg(feature = "serial")]
    serial: SerialPort<'static, Bus>,
    iface: Interface,
    sockets: SocketSet<'static>,
    tcp: SocketHandle,
    udp: SocketHandle,
    /// Output queued for the Link's TCP socket (framed as `0x01` by `link_io`).
    out: Ring<1024>,
    /// Input for the user's program: the Link's `0x02` frames land here (with
    /// the `serial` feature, live CDC-ACM reads are taken directly instead —
    /// see `read_bytes`).
    inp: Ring<256>,
    parser: FrameParser<256>,
    hello: bool,
    lease: u8,
    ms: u64,
}

/// A handle to the started console. Cheap and freely copyable: all state is
/// actually held in a static behind a critical section, so any number of
/// `Console` values observe the same one console.
#[derive(Clone, Copy)]
pub struct Console(());

/// Starts the console: brings up the clocks, the USB device (the Ferric Link,
/// plus CDC-ACM with the `serial` feature) and enables its interrupt. Returns a handle implementing
/// [`embedded_io::Read`] and [`embedded_io::Write`].
///
/// Call this **first thing** in `main`, before touching any other peripheral
/// (in particular before your own call to `Peripherals::take()` or
/// `init_clocks_and_plls`): internally this steals the peripherals it needs
/// rather than taking them, so your own `Peripherals::take()` afterwards still
/// succeeds, and re-running `init_clocks_and_plls` from your own code
/// afterwards is safe (rp-hal detaches `clk_sys`/`clk_ref` from the PLLs
/// before touching them again). If the clocks fail to start, the console
/// stays inactive and every read/write is a harmless no-op — your program
/// still runs.
pub fn start() -> Console {
    bring_up()
}

/// Like [`start`], and also tells the Link what to call this firmware: `name`
/// (conventionally `env!("CARGO_PKG_NAME")` of the calling crate) is what the
/// host shows as the firmware's identity, and [`crate::init_log`] then forwards
/// only the log records whose target is in that crate (as Ferric always did).
pub fn start_named(name: &'static str) -> Console {
    crate::identity::set(name);
    bring_up()
}

fn bring_up() -> Console {
    let Some(usb_clock) = hal_impl::usb_clock() else {
        return Console(());
    };
    let bus = hal_impl::usb_bus(usb_clock);
    let alloc = unsafe { ALLOC.get() }.insert(UsbBusAllocator::new(bus));
    let alloc: &'static UsbBusAllocator<Bus> = alloc;
    let bufs = unsafe { BUFS.get() };
    let metas = unsafe { METAS.get() };

    let mut eth = Ethernet::new(alloc, HOST_MAC, 64, &mut bufs.eth_in, &mut bufs.eth_out);
    #[cfg(feature = "serial")]
    let serial = SerialPort::new(alloc);
    // The USB shape the iPad already mounts as "Ethernet" (IAD + 0xEF/0x02/0x01
    // for the Link); with the `serial` feature the CDC-ACM pair gets its own
    // IAD alongside it.
    let usb = UsbDeviceBuilder::new(alloc, UsbVidPid(0xc0de, 0xcafe))
        .strings(&[StringDescriptors::default()
            .manufacturer("Ferric")
            .product(crate::PRODUCT)
            .serial_number(crate::SERIAL_NUMBER)])
        .unwrap()
        .composite_with_iads()
        .max_packet_size_0(64)
        .unwrap()
        .build();

    let mut cfg = IfConfig::new(EthernetAddress(OUR_MAC).into());
    cfg.random_seed = 0x_f2ff_11cc;
    let mut iface = Interface::new(cfg, &mut eth, Instant::from_micros(0));
    iface.update_ip_addrs(|a| {
        let _ = a.push(IpCidr::new(IpAddress::v4(IP[0], IP[1], IP[2], IP[3]), 24));
    });
    // No default route: the host keeps its other network interfaces (e.g. WiFi) up.

    let mut sockets = SocketSet::new(&mut metas.socks[..]);
    let tcp = sockets.add(tcp::Socket::new(
        tcp::SocketBuffer::new(&mut bufs.tcp_rx[..]),
        tcp::SocketBuffer::new(&mut bufs.tcp_tx[..]),
    ));
    let udp = sockets.add(udp::Socket::new(
        udp::PacketBuffer::new(&mut metas.udp_rx[..], &mut bufs.udp_rx[..]),
        udp::PacketBuffer::new(&mut metas.udp_tx[..], &mut bufs.udp_tx[..]),
    ));
    // Listening from the start: a DHCPDISCOVER or SYN that arrives before the
    // first poll must not be lost.
    let _ = sockets.get_mut::<udp::Socket>(udp).bind(67);
    let _ = sockets.get_mut::<tcp::Socket>(tcp).listen(IpListenEndpoint {
        addr: None,
        port: PORT,
    });

    *unsafe { LINK.get() } = Some(Link {
        usb,
        eth,
        #[cfg(feature = "serial")]
        serial,
        iface,
        sockets,
        tcp,
        udp,
        out: Ring::new(),
        inp: Ring::new(),
        parser: FrameParser::new(),
        hello: false,
        lease: 0,
        ms: 0,
    });

    hal_impl::sof_enable();
    hal_impl::irq_enable();
    Console(())
}

/// Queues `buf` for the Link (drained as `0x01` frames by the interrupt) and,
/// with the `serial` feature, writes it to CDC-ACM directly (best-effort).
/// Never blocks: a full Link queue drops its oldest bytes, and a full CDC-ACM
/// buffer just drops `buf` for that transport (`UsbError::WouldBlock`).
fn write_bytes(buf: &[u8]) {
    critical_section::with(|_| {
        let Some(l) = (unsafe { LINK.get() }).as_mut() else {
            return;
        };
        l.out.push(buf);
        #[cfg(feature = "serial")]
        let _ = l.serial.write(buf);
    });
    // Nudge the interrupt so this write is sent promptly instead of waiting
    // for the next 1 kHz SOF tick.
    hal_impl::pend();
}

/// Returns the bytes the host has sent right now: the Link's buffered input
/// (fed by `link_io`'s `0x02` frames) and, with the `serial` feature, a live
/// CDC-ACM read tried first. Returns `0` when there is nothing — never blocks.
pub(crate) fn read_bytes(buf: &mut [u8]) -> usize {
    critical_section::with(|_| {
        let Some(l) = (unsafe { LINK.get() }).as_mut() else {
            return 0;
        };
        #[cfg(feature = "serial")]
        return read_either(buf, |b| l.serial.read(b).unwrap_or(0), |b| l.inp.take(b));
        #[cfg(not(feature = "serial"))]
        l.inp.take(buf)
    })
}

#[cfg_attr(not(feature = "log"), allow(dead_code))]
struct LineWriter;
impl core::fmt::Write for LineWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        write_bytes(s.as_bytes());
        Ok(())
    }
}

/// Formats `args` and writes it followed by a newline, to both transports.
/// Used by [`crate::init_log`]; also usable directly for a `println!`-style
/// helper of your own.
#[cfg_attr(not(feature = "log"), allow(dead_code))]
pub(crate) fn log_fmt(args: core::fmt::Arguments) {
    use core::fmt::Write;
    let _ = write!(LineWriter, "{}", args);
    write_bytes(b"\n");
}

impl Console {
    /// Non-blocking read: how many bytes were available right now (`0` when
    /// there is nothing). The same as [`embedded_io::Read::read`], without the
    /// `Result`.
    pub fn try_read(&mut self, buf: &mut [u8]) -> usize {
        read_bytes(buf)
    }
}

impl embedded_io::ErrorType for Console {
    type Error = core::convert::Infallible;
}

impl embedded_io::Read for Console {
    /// Never blocks. `Ok(0)` means "nothing available right now", not EOF —
    /// this console has no end of input.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        Ok(read_bytes(buf))
    }
}

impl embedded_io::Write for Console {
    /// Always reports success: writes are best-effort by design (a full
    /// queue on either transport just drops bytes), so there is nothing
    /// meaningful to fail with.
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        write_bytes(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn dispatch_frame(kind: u8, payload: &[u8], inp: &mut Ring<256>, boot: &mut bool) {
    if kind == FrameKind::Input.as_u8() {
        inp.push(payload);
    } else if kind == FrameKind::Control.as_u8() && payload == BOOTSEL_PAYLOAD {
        // The host asked for BOOTSEL without the user touching the button.
        *boot = true;
    }
}

impl Link {
    /// The Link's clock is the host's SOF (1 kHz): it neither steals a
    /// `TIMER` from the user's program nor depends on their clocks being set up.
    fn now(&self) -> Instant {
        Instant::from_millis(self.ms as i64)
    }

    fn poll(&mut self) {
        #[cfg(feature = "serial")]
        self.usb.poll(&mut [&mut self.eth, &mut self.serial]);
        #[cfg(not(feature = "serial"))]
        self.usb.poll(&mut [&mut self.eth]);
        // With the impl's generics spelled out: called as a method, rust-analyzer
        // leaves the receiver uninferred and flags a phantom E0282 (rustc does not).
        if Ethernet::<'static, Bus>::state(&self.eth) == DeviceState::Disconnected {
            let _ = self.eth.connect();
        }
        let now = self.now();
        self.iface.poll(now, &mut self.eth, &mut self.sockets);
        self.dhcp();
        if self.link_io() {
            hal_impl::bootsel();
        }
        self.iface.poll(now, &mut self.eth, &mut self.sockets);
    }

    /// Minimal DHCP server: a single offer, with NO router (option 3) or DNS,
    /// so the host's default route (e.g. its WiFi) is left alone.
    fn dhcp(&mut self) {
        let Link {
            sockets,
            udp,
            lease,
            ..
        } = self;
        let s = sockets.get_mut::<udp::Socket>(*udp);
        if !s.is_open() && s.bind(67).is_err() {
            return;
        }
        let mut req = [0u8; 576];
        let n = match s.recv_slice(&mut req) {
            Ok((n, _)) => n,
            Err(_) => return,
        };
        if n < 240 || req[0] != 1 || req[236..240] != [0x63, 0x82, 0x53, 0x63] {
            return;
        }
        // The options we care about: 53 (type), 50 (requested IP), 54 (server id).
        let (mut mtype, mut want, mut sid) = (0u8, None::<[u8; 4]>, None::<[u8; 4]>);
        let mut i = 240;
        while i + 1 < n {
            let (code, len) = (req[i], usize::from(req[i + 1]));
            if code == 255 {
                break;
            }
            if code == 0 {
                i += 1;
                continue;
            }
            if i + 2 + len > n {
                break;
            }
            let v = &req[i + 2..i + 2 + len];
            match (code, len) {
                (53, 1) => mtype = v[0],
                (50, 4) => want = Some([v[0], v[1], v[2], v[3]]),
                (54, 4) => sid = Some([v[0], v[1], v[2], v[3]]),
                _ => {}
            }
            i += 2 + len;
        }
        if let Some(sid) = sid {
            if sid != IP {
                return; // the client picked a different server
            }
        }
        let in_pool = |a: [u8; 4]| a[..3] == IP[..3] && a[3] >= POOL_LO && a[3] <= POOL_HI;
        let mut offer = [IP[0], IP[1], IP[2], POOL_LO];
        if *lease >= POOL_LO {
            offer[3] = *lease;
        }
        let reply_type = match mtype {
            1 => {
                if let Some(w) = want {
                    if in_pool(w) {
                        offer = w;
                    }
                }
                2 // OFFER
            }
            3 => {
                let asked = want.unwrap_or([req[12], req[13], req[14], req[15]]);
                if in_pool(asked) {
                    offer = asked;
                    *lease = asked[3];
                    5 // ACK
                } else {
                    6 // NAK
                }
            }
            _ => return,
        };

        let mut rep = [0u8; 300];
        rep[0] = 2; // BOOTREPLY
        rep[1] = 1; // ethernet
        rep[2] = 6; // hlen
        rep[4..8].copy_from_slice(&req[4..8]); // xid
        rep[10..12].copy_from_slice(&req[10..12]); // flags (broadcast bit as-is)
        if reply_type == 5 {
            rep[12..16].copy_from_slice(&req[12..16]); // ciaddr
        }
        if reply_type != 6 {
            rep[16..20].copy_from_slice(&offer); // yiaddr
        }
        rep[24..28].copy_from_slice(&req[24..28]); // giaddr
        rep[28..44].copy_from_slice(&req[28..44]); // chaddr
        rep[236..240].copy_from_slice(&[0x63, 0x82, 0x53, 0x63]);
        let mut o = 240;
        let mut put = |code: u8, val: &[u8]| {
            rep[o] = code;
            rep[o + 1] = val.len() as u8;
            rep[o + 2..o + 2 + val.len()].copy_from_slice(val);
            o += 2 + val.len();
        };
        put(53, &[reply_type]);
        put(54, &IP);
        if reply_type != 6 {
            put(51, &7200u32.to_be_bytes());
            put(1, &[255, 255, 255, 0]);
        }
        rep[o] = 255; // end of options (the rest of the 300 bytes stays zero)

        // Where to reply (RFC 2131 §4.1, the same rule `edge-dhcp` uses, which
        // the iPad already accepts): to the relay if there is one, unicast to
        // ciaddr on a renewal without the broadcast flag, else broadcast.
        // Never to yiaddr: the client is not listening on that IP yet.
        let giaddr = [req[24], req[25], req[26], req[27]];
        let ciaddr = [req[12], req[13], req[14], req[15]];
        let to = if giaddr != [0, 0, 0, 0] {
            IpEndpoint::new(
                IpAddress::v4(giaddr[0], giaddr[1], giaddr[2], giaddr[3]),
                67,
            )
        } else if ciaddr != [0, 0, 0, 0] && req[10] & 0x80 == 0 {
            IpEndpoint::new(
                IpAddress::v4(ciaddr[0], ciaddr[1], ciaddr[2], ciaddr[3]),
                68,
            )
        } else {
            IpEndpoint::new(IpAddress::v4(255, 255, 255, 255), 68)
        };
        let _ = s.send_slice(&rep, to);
    }

    /// The Link's TCP service: accepts the host, greets it (`0x04`), and
    /// multiplexes output (`0x01`) and input (`0x02`). Returns `true` when the
    /// host asked for BOOTSEL.
    fn link_io(&mut self) -> bool {
        let Link {
            sockets,
            tcp,
            out,
            inp,
            parser,
            hello,
            ..
        } = self;
        let s = sockets.get_mut::<tcp::Socket>(*tcp);
        match s.state() {
            tcp::State::Closed => {
                *hello = false;
                let _ = s.listen(IpListenEndpoint {
                    addr: None,
                    port: PORT,
                });
                return false;
            }
            tcp::State::Listen | tcp::State::SynSent | tcp::State::SynReceived => return false,
            _ => {}
        }
        // The host closed (or the link dropped): abort and listen again, or the
        // next connection would find nobody on the other end of the port.
        if !s.may_recv() && !s.may_send() {
            s.abort();
            *hello = false;
            return false;
        }
        if !*hello {
            let name = crate::identity::name().as_bytes();
            let head = crate::protocol::encode_header(FrameKind::Hello, name.len() as u16);
            if s.send_capacity() - s.send_queue() >= name.len() + 3 {
                let _ = s.send_slice(&head);
                let _ = s.send_slice(name);
                *hello = true;
            }
        }
        let mut boot = false;
        while s.can_recv() {
            let mut tmp = [0u8; 256];
            let n = s.recv_slice(&mut tmp).unwrap_or(0);
            if n == 0 {
                break;
            }
            for &b in &tmp[..n] {
                if let Some(done) = parser.feed(b) {
                    dispatch_frame(done.kind, parser.payload(), inp, &mut boot);
                }
            }
        }
        if s.state() == tcp::State::CloseWait {
            s.close();
        }
        while !out.is_empty() && s.can_send() {
            let mut tmp = [0u8; 240];
            let n = out.peek(&mut tmp);
            if s.send_capacity() - s.send_queue() < n + 3 {
                break;
            }
            let head = crate::protocol::encode_header(FrameKind::Output, n as u16);
            let _ = s.send_slice(&head);
            let _ = s.send_slice(&tmp[..n]);
            out.discard(n);
        }
        boot
    }
}

/// The whole console lives in this interrupt: the user's program can block in
/// `delay_ms` without dropping a connection. The 1 kHz tick is the host's own
/// start-of-frame.
#[allow(non_snake_case)]
#[interrupt]
fn USBCTRL_IRQ() {
    critical_section::with(|_| {
        let sof = hal_impl::sof_tick();
        if let Some(l) = (unsafe { LINK.get() }).as_mut() {
            if sof {
                l.ms = l.ms.wrapping_add(1);
            }
            l.poll();
        }
    });
}
