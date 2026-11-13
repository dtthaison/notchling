use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

pub const HEAD_BYTES: usize = 4096;
const BLOCK: usize = 1 << 20;

pub enum Line<'a> {
    Whole(&'a [u8]),
    Oversized { head: &'a [u8], len: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scanned {
    pub span: u64,
    pub parsed: u64,
}

struct Pending {
    max_line: usize,
    line: Vec<u8>,
    len: u64,
    over: bool,
}

impl Pending {
    fn push(&mut self, segment: &[u8]) {
        self.len += segment.len() as u64;
        if !self.over && self.line.len() + segment.len() <= self.max_line {
            self.line.extend_from_slice(segment);
            return;
        }
        self.over = true;
        self.line.truncate(HEAD_BYTES);
        let room = HEAD_BYTES.saturating_sub(self.line.len());
        self.line
            .extend_from_slice(&segment[..room.min(segment.len())]);
    }

    fn clear(&mut self) {
        self.line.clear();
        self.len = 0;
        self.over = false;
    }
}

pub fn scan_lines(
    path: &str,
    start: u64,
    end: u64,
    skip_first_line: bool,
    max_line: usize,
    mut each: impl FnMut(Line<'_>),
) -> io::Result<Scanned> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(start))?;
    let mut remaining = end.saturating_sub(start);
    let mut buf = vec![0u8; remaining.min(BLOCK as u64) as usize];
    let mut pending = Pending {
        max_line,
        line: Vec::new(),
        len: 0,
        over: false,
    };
    let mut skipping = skip_first_line;
    let mut scanned = Scanned { span: 0, parsed: 0 };
    let mut at = 0u64;
    while remaining > 0 {
        let want = remaining.min(buf.len() as u64) as usize;
        let n = file.read(&mut buf[..want])?;
        if n == 0 {
            break;
        }
        remaining -= n as u64;
        let block = &buf[..n];
        let mut from = 0;
        while from < n {
            let Some(rel) = block[from..].iter().position(|&b| b == b'\n') else {
                pending.push(&block[from..]);
                break;
            };
            let segment = &block[from..from + rel];
            from += rel + 1;
            scanned.span = at + from as u64;
            if skipping {
                skipping = false;
            } else if pending.len == 0 && segment.is_empty() {
            } else if pending.len == 0 && segment.len() <= max_line {
                scanned.parsed += segment.len() as u64 + 1;
                each(Line::Whole(segment));
            } else {
                pending.push(segment);
                if pending.over {
                    each(Line::Oversized {
                        head: &pending.line,
                        len: pending.len,
                    });
                } else {
                    scanned.parsed += pending.len + 1;
                    each(Line::Whole(&pending.line));
                }
            }
            pending.clear();
        }
        at += n as u64;
    }
    Ok(scanned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[derive(Debug, PartialEq)]
    enum Got {
        Whole(String),
        Big(String, u64),
    }

    fn scan(text: &str, start: u64, skip: bool, cap: usize) -> (Vec<Got>, Scanned) {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("scan-unit-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("a.jsonl");
        fs::write(&path, text).expect("write");
        let mut got = Vec::new();
        let done = scan_lines(
            &path.to_string_lossy(),
            start,
            text.len() as u64,
            skip,
            cap,
            |line| {
                got.push(match line {
                    Line::Whole(l) => Got::Whole(String::from_utf8_lossy(l).into_owned()),
                    Line::Oversized { head, len } => {
                        Got::Big(String::from_utf8_lossy(head).into_owned(), len)
                    }
                })
            },
        )
        .expect("scan");
        let _ = fs::remove_dir_all(&dir);
        (got, done)
    }

    fn whole(s: &str) -> Got {
        Got::Whole(s.into())
    }

    #[test]
    fn lines_come_in_order_and_a_half_line_is_held_back() {
        let (got, done) = scan("one\n\ntwo\nthr", 0, false, 10);
        assert_eq!(got, vec![whole("one"), whole("two")]);
        assert_eq!(done, Scanned { span: 9, parsed: 8 });
    }

    #[test]
    fn a_read_from_mid_line_drops_the_first_partial_line() {
        let (got, done) = scan("abcdef\nxyz\n", 3, true, 10);
        assert_eq!(got, vec![whole("xyz")]);
        assert_eq!(done.span, 8, "from byte 3 through the last newline");
        let (got, done) = scan("abcdef", 3, true, 10);
        assert!(got.is_empty());
        assert_eq!(done.span, 0);
    }

    #[test]
    fn a_line_over_the_cap_is_not_held_only_its_head_and_length() {
        let big = "x".repeat(HEAD_BYTES * 3);
        let (got, done) = scan(&format!("a\n{big}\nb\n"), 0, false, 100);
        assert_eq!(
            got,
            vec![
                whole("a"),
                Got::Big("x".repeat(HEAD_BYTES), (HEAD_BYTES * 3) as u64),
                whole("b")
            ]
        );
        assert_eq!(done.parsed, 4, "only a and b were handed over");
        assert_eq!(done.span, (HEAD_BYTES * 3 + 5) as u64);
    }

    #[test]
    fn a_line_exactly_at_the_cap_is_whole() {
        let (got, _) = scan("12345\n123456\n", 0, false, 5);
        assert_eq!(got, vec![whole("12345"), Got::Big("123456".into(), 6)]);
    }

    #[test]
    fn a_line_longer_than_a_block_is_put_together() {
        let long = "y".repeat(BLOCK * 2 + 17);
        let (got, done) = scan(&format!("{long}\nend\n"), 0, false, BLOCK * 3);
        assert_eq!(got, vec![Got::Whole(long.clone()), whole("end")]);
        assert_eq!(done.span, (long.len() + 5) as u64);
        let (got, _) = scan(&format!("{long}\nend\n"), 0, false, 100);
        assert_eq!(
            got,
            vec![
                Got::Big("y".repeat(HEAD_BYTES), long.len() as u64),
                whole("end")
            ]
        );
    }
}
