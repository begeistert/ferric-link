//! The Ferric Link wire protocol: a tiny framing format multiplexing console
//! data and control messages over one stream (a TCP socket for the Link, in
//! the same shape a future transport could reuse).
//!
//! A frame is `kind(1 byte) + len(2 bytes, little-endian) + payload(len bytes)`.
//! This module is pure and `no_std`-and-`std` agnostic: it does no I/O, so it
//! is exercised directly by `cargo test` on the host, with no HAL feature.

/// The kind byte of a Link frame.
///
/// Values and direction match the protocol Ferric's iPad app speaks today
/// (see `FerricLink.swift`): this is a wire format, not free to renumber.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameKind {
    /// Pico → host: program output (what the console's `Write` half sends).
    Output = 0x01,
    /// Host → Pico: input for the program (what the console's `Read` half returns).
    Input = 0x02,
    /// Host → Pico: control frame. The only payload defined today is
    /// [`BOOTSEL_PAYLOAD`], which reboots the board into USB boot (BOOTSEL)
    /// mode without touching the physical button.
    Control = 0x03,
    /// Pico → host: hello / firmware identity, sent once when a connection opens.
    Hello = 0x04,
}

impl FrameKind {
    /// Decodes a kind byte, or `None` for a byte the protocol does not define
    /// (a parser should skip such a frame's payload and keep going, not desync).
    pub const fn from_u8(b: u8) -> Option<Self> {
        match b {
            0x01 => Some(Self::Output),
            0x02 => Some(Self::Input),
            0x03 => Some(Self::Control),
            0x04 => Some(Self::Hello),
            _ => None,
        }
    }

    /// The kind's byte on the wire.
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// The control payload that reboots the board into USB boot (BOOTSEL) mode.
pub const BOOTSEL_PAYLOAD: &[u8] = b"bootsel";

/// Encodes a frame header (kind + little-endian length) for `payload`. The
/// caller writes this, then `payload`, to the transport.
pub const fn encode_header(kind: FrameKind, payload_len: u16) -> [u8; 3] {
    [
        kind.as_u8(),
        (payload_len & 0xff) as u8,
        (payload_len >> 8) as u8,
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Kind,
    LenLo,
    LenHi,
    Payload,
}

/// A frame that just finished arriving. Its payload is available from
/// [`FrameParser::payload`] until the next call to [`FrameParser::feed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompletedFrame {
    /// The kind byte as received (a [`FrameKind`] value, or one the protocol does not define).
    pub kind: u8,
    /// The payload length as declared on the wire — this can exceed the
    /// parser's buffer `N`; the frame is still consumed correctly (framing
    /// never desyncs), but only `N` bytes of it are kept, see `payload()`.
    pub declared_len: usize,
}

/// Incremental, allocation-free parser for the frame format above. Feed it
/// one byte at a time as it arrives from a transport (it never blocks and
/// never looks ahead), and it reports when a frame has just completed.
///
/// `N` bounds how many payload bytes the parser retains; a frame longer than
/// `N` is still parsed correctly (the length on the wire is honored so the
/// next frame's header is found at the right offset), its extra bytes are
/// just not kept.
pub struct FrameParser<const N: usize> {
    state: State,
    kind: u8,
    len: usize,
    got: usize,
    buf: [u8; N],
}

impl<const N: usize> Default for FrameParser<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> FrameParser<N> {
    /// An empty parser, waiting for a frame's kind byte.
    pub const fn new() -> Self {
        Self {
            state: State::Kind,
            kind: 0,
            len: 0,
            got: 0,
            buf: [0; N],
        }
    }

    /// Feeds one byte. Returns `Some(_)` exactly when that byte completed a
    /// frame; read [`Self::payload`] right away, before feeding more bytes.
    pub fn feed(&mut self, b: u8) -> Option<CompletedFrame> {
        match self.state {
            State::Kind => {
                self.kind = b;
                self.state = State::LenLo;
                None
            }
            State::LenLo => {
                self.len = b as usize;
                self.state = State::LenHi;
                None
            }
            State::LenHi => {
                self.len |= (b as usize) << 8;
                self.got = 0;
                if self.len == 0 {
                    self.state = State::Kind;
                    Some(CompletedFrame {
                        kind: self.kind,
                        declared_len: 0,
                    })
                } else {
                    self.state = State::Payload;
                    None
                }
            }
            State::Payload => {
                if self.got < N {
                    self.buf[self.got] = b;
                }
                self.got += 1;
                if self.got >= self.len {
                    self.state = State::Kind;
                    Some(CompletedFrame {
                        kind: self.kind,
                        declared_len: self.len,
                    })
                } else {
                    None
                }
            }
        }
    }

    /// The payload of the frame that just completed (up to `N` bytes of it).
    pub fn payload(&self) -> &[u8] {
        &self.buf[..self.got.min(N)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all<const N: usize>(p: &mut FrameParser<N>, bytes: &[u8]) -> Vec<CompletedFrame> {
        bytes.iter().filter_map(|&b| p.feed(b)).collect()
    }

    #[test]
    fn encodes_header_little_endian() {
        assert_eq!(encode_header(FrameKind::Output, 0x0102), [0x01, 0x02, 0x01]);
        assert_eq!(encode_header(FrameKind::Hello, 0), [0x04, 0x00, 0x00]);
    }

    #[test]
    fn frame_kind_round_trips() {
        for k in [
            FrameKind::Output,
            FrameKind::Input,
            FrameKind::Control,
            FrameKind::Hello,
        ] {
            assert_eq!(FrameKind::from_u8(k.as_u8()), Some(k));
        }
        assert_eq!(FrameKind::from_u8(0xEE), None);
    }

    #[test]
    fn parses_one_frame() {
        let mut p: FrameParser<64> = FrameParser::new();
        let mut frame = vec![0x01, 3, 0];
        frame.extend_from_slice(b"hi!");
        let done = feed_all(&mut p, &frame);
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].kind, 0x01);
        assert_eq!(done[0].declared_len, 3);
        assert_eq!(p.payload(), b"hi!");
    }

    #[test]
    fn parses_zero_length_frame() {
        let mut p: FrameParser<64> = FrameParser::new();
        let done = feed_all(&mut p, &[0x04, 0, 0]);
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].kind, 0x04);
        assert_eq!(done[0].declared_len, 0);
        assert_eq!(p.payload(), b"");
    }

    #[test]
    fn parses_several_frames_back_to_back() {
        let mut p: FrameParser<64> = FrameParser::new();
        let mut bytes = vec![0x02, 1, 0, b'a'];
        bytes.extend_from_slice(&[0x02, 2, 0, b'b', b'c']);
        let done = feed_all(&mut p, &bytes);
        assert_eq!(done.len(), 2);
        assert_eq!(done[1].declared_len, 2);
        assert_eq!(p.payload(), b"bc");
    }

    #[test]
    fn resyncs_after_bytes_split_across_calls() {
        // Bytes can arrive one at a time from a transport; the parser must not
        // need to see a whole frame at once.
        let mut p: FrameParser<8> = FrameParser::new();
        assert!(p.feed(0x03).is_none());
        assert!(p.feed(7).is_none());
        assert!(p.feed(0).is_none());
        // all but the last payload byte: the frame is not complete yet
        for &b in &b"bootsel"[..6] {
            assert!(p.feed(b).is_none());
        }
        // the last payload byte completes the frame
        let done = p.feed(b"bootsel"[6]).expect("frame should be complete");
        assert_eq!(done.kind, 0x03);
        assert_eq!(p.payload(), BOOTSEL_PAYLOAD);
        // the header of the NEXT frame follows immediately, undisturbed
        assert!(p.feed(0x04).is_none());
    }

    #[test]
    fn payload_longer_than_buffer_is_truncated_but_framing_stays_correct() {
        let mut p: FrameParser<4> = FrameParser::new();
        let mut bytes = vec![0x01, 6, 0];
        bytes.extend_from_slice(b"abcdef"); // 6 bytes, buffer only holds 4
        bytes.extend_from_slice(&[0x04, 0, 0]); // a second, well-formed frame
        let mut first_len = None;
        let mut first_payload = Vec::new();
        let mut second_kind = None;
        for &b in &bytes {
            if let Some(done) = p.feed(b) {
                // capture right away: payload() is only valid until the next feed()
                if first_len.is_none() {
                    first_len = Some(done.declared_len);
                    first_payload = p.payload().to_vec();
                } else {
                    second_kind = Some(done.kind);
                }
            }
        }
        assert_eq!(first_len, Some(6));
        assert_eq!(first_payload, b"abcd"); // only the kept prefix
        assert_eq!(second_kind, Some(0x04)); // the next frame's header was found correctly
    }

    #[test]
    fn bootsel_control_frame_matches_the_published_payload() {
        let mut p: FrameParser<16> = FrameParser::new();
        let mut bytes = vec![FrameKind::Control.as_u8(), BOOTSEL_PAYLOAD.len() as u8, 0];
        bytes.extend_from_slice(BOOTSEL_PAYLOAD);
        let done = feed_all(&mut p, &bytes);
        assert_eq!(done[0].kind, FrameKind::Control.as_u8());
        assert_eq!(p.payload(), BOOTSEL_PAYLOAD);
    }
}
