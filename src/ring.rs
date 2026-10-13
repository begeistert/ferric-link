//! A fixed-capacity byte ring buffer that never blocks a writer: once full, a
//! `push` discards the oldest bytes to make room for the newest ones. This is
//! the buffering strategy behind the console's non-blocking writes (losing
//! old, already-drained-slowly output is preferable to ever stalling the
//! caller's program).
//!
//! Pure and allocation-free, so it is exercised directly by `cargo test`.

pub struct Ring<const N: usize> {
    buf: [u8; N],
    head: usize,
    len: usize,
}

impl<const N: usize> Default for Ring<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Ring<N> {
    pub const fn new() -> Self {
        Self {
            buf: [0; N],
            head: 0,
            len: 0,
        }
    }

    /// Appends `src`, discarding the oldest bytes first if it does not fit.
    pub fn push(&mut self, src: &[u8]) {
        for &b in src {
            if self.len == N {
                self.head = (self.head + 1) % N;
                self.len -= 1;
            }
            self.buf[(self.head + self.len) % N] = b;
            self.len += 1;
        }
    }

    /// Appends only what fits — nothing is ever discarded — and returns how
    /// many bytes were taken. For byte pipes where losing data corrupts the
    /// protocol (the probe's RSP stream): the caller retries the remainder
    /// once the drain side has made room.
    pub fn push_some(&mut self, src: &[u8]) -> usize {
        let n = src.len().min(N - self.len);
        for &b in &src[..n] {
            self.buf[(self.head + self.len) % N] = b;
            self.len += 1;
        }
        n
    }

    /// Bytes of free capacity right now.
    pub fn free(&self) -> usize {
        N - self.len
    }

    /// Copies up to `dst.len()` of the oldest buffered bytes into `dst`,
    /// without removing them. Returns how many were copied.
    pub fn peek(&self, dst: &mut [u8]) -> usize {
        let n = self.len.min(dst.len());
        for (i, d) in dst[..n].iter_mut().enumerate() {
            *d = self.buf[(self.head + i) % N];
        }
        n
    }

    /// Removes the oldest `n` bytes (clamped to what is buffered).
    pub fn discard(&mut self, n: usize) {
        let n = n.min(self.len);
        self.head = (self.head + n) % N;
        self.len -= n;
    }

    /// `peek` followed by `discard`: removes and returns the bytes copied.
    pub fn take(&mut self, dst: &mut [u8]) -> usize {
        let n = self.peek(dst);
        self.discard(n);
        n
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Only exercised by unit tests today, but part of the type's natural API.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.len
    }
}

impl<const N: usize> core::fmt::Write for Ring<N> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.push(s.as_bytes());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_then_take_round_trips() {
        let mut r: Ring<8> = Ring::new();
        r.push(b"abc");
        let mut out = [0u8; 8];
        let n = r.take(&mut out);
        assert_eq!(&out[..n], b"abc");
        assert!(r.is_empty());
    }

    #[test]
    fn peek_does_not_consume() {
        let mut r: Ring<8> = Ring::new();
        r.push(b"xy");
        let mut out = [0u8; 8];
        assert_eq!(r.peek(&mut out), 2);
        assert_eq!(r.len(), 2); // still there
        assert_eq!(r.peek(&mut out), 2); // peeking again gives the same bytes
    }

    #[test]
    fn full_ring_discards_the_oldest_bytes_first() {
        let mut r: Ring<4> = Ring::new();
        r.push(b"abcd");
        r.push(b"ef"); // overflows by 2: "ab" is the oldest and goes first
        let mut out = [0u8; 4];
        let n = r.take(&mut out);
        assert_eq!(&out[..n], b"cdef");
    }

    #[test]
    fn never_grows_past_capacity_even_with_one_huge_push() {
        let mut r: Ring<4> = Ring::new();
        r.push(b"abcdefgh"); // 8 bytes into a 4-byte ring in one call
        assert_eq!(r.len(), 4);
        let mut out = [0u8; 4];
        let n = r.take(&mut out);
        assert_eq!(&out[..n], b"efgh"); // only the newest 4 bytes survive
    }

    #[test]
    fn push_some_never_discards_and_reports_shortfall() {
        let mut r: Ring<4> = Ring::new();
        assert_eq!(r.push_some(b"abc"), 3);
        assert_eq!(r.free(), 1);
        assert_eq!(r.push_some(b"def"), 1); // only 'd' fit
        assert_eq!(r.push_some(b"x"), 0); // full: nothing taken
        let mut out = [0u8; 4];
        let n = r.take(&mut out);
        assert_eq!(&out[..n], b"abcd"); // nothing dropped, 'e' stayed out
        assert_eq!(r.free(), 4);
        assert_eq!(r.push_some(b"ef"), 2); // the caller retries the rest
    }

    #[test]
    fn wraps_around_the_buffer_boundary() {
        let mut r: Ring<4> = Ring::new();
        r.push(b"ab");
        let mut out = [0u8; 4];
        r.take(&mut out[..1]); // discard 'a', head now at index 1
        r.push(b"cde"); // wraps past the end of the backing array
        let n = r.take(&mut out);
        assert_eq!(&out[..n], b"bcde");
    }

    #[test]
    fn implements_fmt_write_for_use_with_write_macro() {
        use core::fmt::Write;
        let mut r: Ring<32> = Ring::new();
        write!(r, "n={}", 42).unwrap();
        let mut out = [0u8; 32];
        let n = r.take(&mut out);
        assert_eq!(&out[..n], b"n=42");
    }
}
