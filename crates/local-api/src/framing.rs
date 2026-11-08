//! Lines on a stream: UTF-8 JSON, `\n` ended, at most [`MAX_LINE`] bytes. A longer line closes the
//! connection, so a client can't make the daemon hold an unbounded buffer.

use serde_json::Value;
use std::io::{self, Read, Write};
use std::time::Instant;

/// The longest line either side accepts, not counting its `\n`: 1 MiB.
pub const MAX_LINE: usize = 1 << 20;

const CHUNK: usize = 64 * 1024;

/// Why no line came.
#[derive(Debug)]
pub enum FrameError {
    /// The line passed the cap before its `\n`.
    TooLong,
    /// The stream ended in the middle of a line.
    PartialAtEof,
    /// A read failed. A read timeout (`WouldBlock` or `TimedOut`) keeps what was read so far, so the
    /// caller may check its deadlines and call again.
    Io(io::Error),
}

impl FrameError {
    /// A read timeout: no data yet, try again.
    pub fn is_timeout(&self) -> bool {
        matches!(self, FrameError::Io(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut))
    }
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::TooLong => write!(f, "a line over {MAX_LINE} bytes"),
            FrameError::PartialAtEof => write!(f, "the stream ended inside a line"),
            FrameError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// Splits a stream into lines, keeping a partial line across read timeouts.
pub struct LineReader<R> {
    inner: R,
    buf: Vec<u8>,
    /// Bytes of `buf` already searched for `\n`.
    scanned: usize,
    max: usize,
    /// When the line now in `buf` got its first byte.
    started: Option<Instant>,
}

impl<R: Read> LineReader<R> {
    pub fn new(inner: R) -> LineReader<R> {
        LineReader::with_max(inner, MAX_LINE)
    }

    /// A reader with another cap, for tests.
    pub fn with_max(inner: R, max: usize) -> LineReader<R> {
        LineReader {
            inner,
            buf: Vec::new(),
            scanned: 0,
            max,
            started: None,
        }
    }

    /// The next line without its `\n`; `None` at a clean end of stream.
    pub fn next_line(&mut self) -> Result<Option<Vec<u8>>, FrameError> {
        loop {
            if let Some(pos) = self.buf[self.scanned..].iter().position(|b| *b == b'\n') {
                let end = self.scanned + pos;
                if end > self.max {
                    return Err(FrameError::TooLong);
                }
                let line = self.buf[..end].to_vec();
                self.buf.drain(..=end);
                self.scanned = 0;
                self.started = (!self.buf.is_empty()).then(Instant::now);
                return Ok(Some(line));
            }
            self.scanned = self.buf.len();
            if self.buf.len() > self.max {
                return Err(FrameError::TooLong);
            }
            let mut chunk = [0u8; CHUNK];
            match self.inner.read(&mut chunk) {
                Ok(0) if self.buf.is_empty() => return Ok(None),
                Ok(0) => return Err(FrameError::PartialAtEof),
                Ok(n) => {
                    if self.buf.is_empty() {
                        self.started = Some(Instant::now());
                    }
                    self.buf.extend_from_slice(&chunk[..n]);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(FrameError::Io(e)),
            }
        }
    }

    /// When the unfinished line now buffered began; `None` when nothing is buffered.
    pub fn partial_since(&self) -> Option<Instant> {
        self.started
    }

    pub fn get_ref(&self) -> &R {
        &self.inner
    }
}

/// The line for `value`, `\n` included, or `None` when it would pass the cap.
pub fn encode_line(value: &Value) -> Option<Vec<u8>> {
    let mut bytes = serde_json::to_vec(value).ok()?;
    if bytes.len() > MAX_LINE {
        return None;
    }
    bytes.push(b'\n');
    Some(bytes)
}

/// Writes one line.
pub fn write_line(out: &mut impl Write, value: &Value) -> io::Result<()> {
    let bytes = encode_line(value)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "line over the size cap"))?;
    out.write_all(&bytes)?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::VecDeque;

    /// A reader that hands out the given pieces one per read, with a timeout between them when the
    /// piece is `None`.
    struct Pieces(VecDeque<Option<Vec<u8>>>);

    impl Read for Pieces {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            match self.0.pop_front() {
                None => Ok(0),
                Some(None) => Err(io::Error::new(io::ErrorKind::WouldBlock, "timeout")),
                Some(Some(mut piece)) => {
                    let n = piece.len().min(out.len());
                    out[..n].copy_from_slice(&piece[..n]);
                    if n < piece.len() {
                        piece.drain(..n);
                        self.0.push_front(Some(piece));
                    }
                    Ok(n)
                }
            }
        }
    }

    fn pieces(list: &[Option<&[u8]>]) -> Pieces {
        Pieces(list.iter().map(|p| p.map(|b| b.to_vec())).collect())
    }

    #[test]
    fn splits_lines_and_ends_cleanly() {
        let mut r = LineReader::new(&b"{\"a\":1}\n{\"b\":2}\n"[..]);
        assert_eq!(r.next_line().unwrap().unwrap(), b"{\"a\":1}");
        assert_eq!(r.next_line().unwrap().unwrap(), b"{\"b\":2}");
        assert!(r.next_line().unwrap().is_none());
    }

    #[test]
    fn a_partial_line_survives_a_timeout_and_completes() {
        let mut r = LineReader::new(pieces(&[Some(b"{\"id\":"), None, Some(b"1}\n")]));
        let first = r.next_line().unwrap_err();
        assert!(first.is_timeout());
        assert!(r.partial_since().is_some());
        assert_eq!(r.next_line().unwrap().unwrap(), b"{\"id\":1}");
        assert!(r.partial_since().is_none());
    }

    #[test]
    fn a_partial_line_at_the_end_is_an_error() {
        let mut r = LineReader::new(&b"{\"id\":1"[..]);
        assert!(matches!(r.next_line(), Err(FrameError::PartialAtEof)));
    }

    #[test]
    fn a_line_at_the_cap_passes_and_one_over_fails() {
        let mut at = vec![b'x'; 16];
        at.push(b'\n');
        let mut r = LineReader::with_max(&at[..], 16);
        assert_eq!(r.next_line().unwrap().unwrap().len(), 16);

        let mut over = vec![b'x'; 17];
        over.push(b'\n');
        let mut r = LineReader::with_max(&over[..], 16);
        assert!(matches!(r.next_line(), Err(FrameError::TooLong)));
    }

    #[test]
    fn an_endless_line_stops_at_the_cap_without_reading_on() {
        // A reader that never ends: the cap must stop it, not the end of the stream.
        let endless = std::io::repeat(b'x');
        let mut r = LineReader::new(endless);
        assert!(matches!(r.next_line(), Err(FrameError::TooLong)));
        assert!(r.buf.len() <= MAX_LINE + CHUNK);
    }

    #[test]
    fn the_real_cap_is_one_mebibyte() {
        let mut big = vec![b' '; MAX_LINE + 1];
        big.push(b'\n');
        assert!(matches!(
            LineReader::new(&big[..]).next_line(),
            Err(FrameError::TooLong)
        ));
        let mut fits = vec![b' '; MAX_LINE];
        fits.push(b'\n');
        assert_eq!(
            LineReader::new(&fits[..])
                .next_line()
                .unwrap()
                .unwrap()
                .len(),
            MAX_LINE
        );
    }

    #[test]
    fn garbage_bytes_still_frame_as_a_line() {
        // Framing only splits; the JSON check is the server's (a bad_request reply).
        let mut r = LineReader::new(&b"\xff\x00garbage\n"[..]);
        assert_eq!(r.next_line().unwrap().unwrap(), b"\xff\x00garbage");
    }

    #[test]
    fn encode_refuses_an_oversize_value() {
        let big = json!("x".repeat(MAX_LINE));
        assert!(encode_line(&big).is_none());
        let line = encode_line(&json!({"id": 1})).unwrap();
        assert_eq!(line.last(), Some(&b'\n'));
    }
}
