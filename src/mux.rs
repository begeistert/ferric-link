//! The read side of [`crate::Console`] has two transports (CDC-ACM and the
//! Link) and one `read()` call: this is the policy deciding whose bytes come
//! back. It is a plain function over two byte sources so it can be tested
//! without any USB stack.

/// Tries `first`, and only if it produced nothing, tries `second`. Both `try_*`
/// closures follow the crate's non-blocking convention: return the number of
/// bytes written into `buf` right now, `0` when the transport has none.
///
/// A fixed order (rather than e.g. round-robin) keeps behaviour predictable
/// for a debugging console; either source that stays empty never delays the
/// other's data by more than one call.
pub fn read_either<A, B>(buf: &mut [u8], mut try_first: A, mut try_second: B) -> usize
where
    A: FnMut(&mut [u8]) -> usize,
    B: FnMut(&mut [u8]) -> usize,
{
    let n = try_first(buf);
    if n > 0 {
        return n;
    }
    try_second(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_zero_when_both_are_empty() {
        let mut buf = [0u8; 8];
        assert_eq!(read_either(&mut buf, |_| 0, |_| 0), 0);
    }

    #[test]
    fn prefers_the_first_source_when_it_has_data() {
        let mut buf = [0u8; 8];
        let n = read_either(
            &mut buf,
            |b| {
                b[..3].copy_from_slice(b"abc");
                3
            },
            |_| panic!("second source must not be consulted"),
        );
        assert_eq!(&buf[..n], b"abc");
    }

    #[test]
    fn falls_back_to_the_second_source_when_the_first_is_empty() {
        let mut buf = [0u8; 8];
        let n = read_either(
            &mut buf,
            |_| 0,
            |b| {
                b[..2].copy_from_slice(b"xy");
                2
            },
        );
        assert_eq!(&buf[..n], b"xy");
    }

    #[test]
    fn a_source_that_always_empties_never_starves_the_other() {
        // Draining `first` down to 0 must not require special-casing by the
        // caller: the very next call already falls through to `second`.
        let mut calls = 0;
        let mut buf = [0u8; 4];
        for _ in 0..3 {
            let n = read_either(
                &mut buf,
                |_| 0,
                |b| {
                    calls += 1;
                    b[0] = b'!';
                    1
                },
            );
            assert_eq!(n, 1);
        }
        assert_eq!(calls, 3);
    }
}
