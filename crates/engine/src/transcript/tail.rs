use super::scan::{scan_lines, Line};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;

pub const TAIL_BYTES: u64 = 2_000_000;
pub const CHAT_TAIL_BYTES: u64 = 4_000_000;
pub const MAX_LINE_BYTES: usize = 100_000;
pub const LONG_LINE_SCAN: u64 = 64_000_000;

pub static PARSED_BYTES: AtomicU64 = AtomicU64::new(0);

pub fn read_chunk(
    path: &str,
    start: u64,
    end: u64,
    skip_first_line: bool,
) -> io::Result<(Vec<Vec<u8>>, u64)> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(start))?;
    let mut data = Vec::new();
    file.take(end.saturating_sub(start))
        .read_to_end(&mut data)?;
    let Some(last) = data.iter().rposition(|&b| b == b'\n') else {
        return Ok((Vec::new(), 0));
    };
    let mut from = 0;
    if skip_first_line {
        if let Some(first) = data.iter().position(|&b| b == b'\n') {
            from = first + 1;
        }
    }
    let lines = if from <= last {
        data[from..last]
            .split(|&b| b == b'\n')
            .filter(|l| !l.is_empty())
            .map(<[u8]>::to_vec)
            .collect()
    } else {
        Vec::new()
    };
    Ok((lines, last as u64 + 1))
}

pub fn read_lines(path: &str, max_bytes: u64) -> io::Result<Vec<String>> {
    let mut file = File::open(path)?;
    let size = file.seek(SeekFrom::End(0))?;
    let start = size.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start))?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    let mut lines: Vec<String> = data
        .split(|&b| b == b'\n')
        .filter(|l| !l.is_empty())
        .map(|l| String::from_utf8_lossy(l).into_owned())
        .collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    Ok(lines)
}

pub fn tail_start(path: &str, size: u64, budget: u64, scan_limit: u64) -> io::Result<u64> {
    tail_start_with(path, size, budget, scan_limit, MAX_LINE_BYTES)
}

pub fn tail_start_with(
    path: &str,
    size: u64,
    budget: u64,
    scan_limit: u64,
    max_line: usize,
) -> io::Result<u64> {
    const BLOCK: u64 = 1 << 20;
    if size <= budget {
        return Ok(0);
    }
    let after = |line_end: u64| if line_end >= size { size } else { line_end + 1 };
    let mut file = File::open(path)?;
    let floor = size.saturating_sub(scan_limit);
    let mut kept = 0u64;
    let mut line_end = size;
    let mut pos = size;
    let mut buf = vec![0u8; BLOCK as usize];
    while pos > floor {
        let from = pos.saturating_sub(BLOCK).max(floor);
        let len = (pos - from) as usize;
        file.seek(SeekFrom::Start(from))?;
        file.read_exact(&mut buf[..len])?;
        for i in (0..len).rev() {
            if buf[i] != b'\n' {
                continue;
            }
            let newline = from + i as u64;
            let length = line_end - (newline + 1);
            if length > 0 && length <= max_line as u64 {
                kept += length + 1;
                if kept > budget {
                    return Ok(after(line_end));
                }
            }
            line_end = newline;
        }
        pos = from;
    }
    if pos > 0 {
        return Ok(after(line_end));
    }
    if line_end > max_line as u64 || kept + line_end < budget {
        Ok(0)
    } else {
        Ok(after(line_end))
    }
}

pub trait LineParser: Default {
    fn consume(&mut self, line: &[u8]);

    fn consume_oversized(&mut self, _head: &[u8], _len: u64) {}
}

struct Cursor<P, O> {
    inode: u64,
    offset: u64,
    modified: SystemTime,
    size: u64,
    parser: P,
    output: O,
    used: u64,
}

struct State<P, O> {
    cursors: HashMap<String, Cursor<P, O>>,
    clock: u64,
}

pub struct Cursors<P: LineParser, O: Clone> {
    tail_bytes: u64,
    limit: usize,
    long_line_scan: u64,
    line_cap: Option<usize>,
    render: Box<dyn Fn(&P) -> O + Send + Sync>,
    state: Mutex<State<P, O>>,
    parsed: AtomicU64,
}

#[cfg(unix)]
fn inode_of(meta: &fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::ino(meta)
}

#[cfg(not(unix))]
fn inode_of(_meta: &fs::Metadata) -> u64 {
    0
}

impl<P: LineParser, O: Clone> Cursors<P, O> {
    pub fn new(
        tail_bytes: u64,
        limit: usize,
        render: impl Fn(&P) -> O + Send + Sync + 'static,
    ) -> Self {
        Cursors {
            tail_bytes,
            limit,
            long_line_scan: 0,
            line_cap: None,
            render: Box::new(render),
            state: Mutex::new(State {
                cursors: HashMap::new(),
                clock: 0,
            }),
            parsed: AtomicU64::new(0),
        }
    }

    pub fn parsed_bytes(&self) -> u64 {
        self.parsed.load(Ordering::Relaxed)
    }

    fn count_parsed(&self, bytes: u64) {
        self.parsed.fetch_add(bytes, Ordering::Relaxed);
        PARSED_BYTES.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn long_lines_free(mut self, scan_limit: u64) -> Self {
        self.long_line_scan = scan_limit;
        self
    }

    pub fn skipping_lines_over(mut self, max_line: usize, scan_limit: u64) -> Self {
        self.line_cap = Some(max_line);
        self.long_line_scan = scan_limit;
        self
    }

    fn fits_the_budget(&self, path: &str, size: u64, offset: u64) -> bool {
        self.long_line_scan > 0
            && tail_start_with(
                path,
                size,
                self.tail_bytes,
                self.long_line_scan,
                self.line_cap.unwrap_or(MAX_LINE_BYTES),
            )
            .is_ok_and(|start| start <= offset)
    }

    fn feed(&self, parser: &mut P, path: &str, start: u64, end: u64, cut: bool) -> io::Result<u64> {
        let Some(cap) = self.line_cap else {
            let (lines, consumed) = read_chunk(path, start, end, cut)?;
            self.count_parsed(consumed);
            for line in &lines {
                parser.consume(line);
            }
            return Ok(consumed);
        };
        let done = scan_lines(path, start, end, cut, cap, |line| match line {
            Line::Whole(line) => parser.consume(line),
            Line::Oversized { head, len } => parser.consume_oversized(head, len),
        })?;
        self.count_parsed(done.parsed);
        Ok(done.span)
    }

    pub fn read(&self, path: &str) -> Option<O> {
        let meta = fs::metadata(path).ok()?;
        let modified = meta.modified().ok()?;
        let size = meta.len();
        let inode = inode_of(&meta);

        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let state = &mut *guard;
        state.clock += 1;
        let clock = state.clock;

        let reusable = match state.cursors.get(path) {
            Some(c)
                if c.inode == inode
                    && c.offset <= size
                    && (size - c.offset <= self.tail_bytes
                        || self.fits_the_budget(path, size, c.offset)) =>
            {
                if c.modified == modified && c.size == size {
                    let output = c.output.clone();
                    if let Some(c) = state.cursors.get_mut(path) {
                        c.used = clock;
                    }
                    return Some(output);
                }
                true
            }
            _ => false,
        };

        if reusable {
            if let Some(mut cursor) = state.cursors.remove(path) {
                if size > cursor.offset {
                    if let Ok(consumed) =
                        self.feed(&mut cursor.parser, path, cursor.offset, size, false)
                    {
                        cursor.offset += consumed;
                    }
                }
                cursor.modified = modified;
                cursor.size = size;
                cursor.output = (self.render)(&cursor.parser);
                cursor.used = clock;
                let output = cursor.output.clone();
                state.cursors.insert(path.to_string(), cursor);
                return Some(output);
            }
        }

        let aligned = (self.long_line_scan > 0)
            .then(|| {
                let cap = self.line_cap.unwrap_or(MAX_LINE_BYTES);
                tail_start_with(path, size, self.tail_bytes, self.long_line_scan, cap).ok()
            })
            .flatten();
        let (start, cut) = match aligned {
            Some(start) => (start, false),
            None => {
                let start = size.saturating_sub(self.tail_bytes);
                (start, start > 0)
            }
        };
        let mut parser = P::default();
        let consumed = self.feed(&mut parser, path, start, size, cut).ok()?;
        let output = (self.render)(&parser);
        if !state.cursors.contains_key(path) && state.cursors.len() >= self.limit {
            let oldest = state
                .cursors
                .iter()
                .min_by_key(|(_, c)| c.used)
                .map(|(k, _)| k.clone());
            if let Some(oldest) = oldest {
                state.cursors.remove(&oldest);
            }
        }
        state.cursors.insert(
            path.to_string(),
            Cursor {
                inode,
                offset: start + consumed,
                modified,
                size,
                parser,
                output: output.clone(),
                used: clock,
            },
        );
        Some(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_chunk_holds_back_a_half_line() {
        let dir = std::env::temp_dir().join(format!("tail-unit-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("a.jsonl");
        fs::write(&path, "one\ntwo\nthr").expect("write");
        let p = path.to_string_lossy().into_owned();
        let (lines, consumed) = read_chunk(&p, 0, 11, false).expect("chunk");
        assert_eq!(lines, vec![b"one".to_vec(), b"two".to_vec()]);
        assert_eq!(consumed, 8);
        let lines = read_lines(&p, 6).expect("lines");
        assert_eq!(
            lines,
            vec!["thr".to_string()],
            "the cut line is dropped, the unfinished last line is kept"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn long_lines_do_not_use_the_tail_budget() {
        let dir = std::env::temp_dir().join(format!("tail-long-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("a.jsonl");
        let big = "x".repeat(MAX_LINE_BYTES + 1);
        let text = format!("old\nprompt\n{big}\n{big}\n{big}\nanswer\n");
        fs::write(&path, &text).expect("write");
        let p = path.to_string_lossy().into_owned();
        let size = text.len() as u64;
        let start = |budget| tail_start(&p, size, budget, u64::MAX).expect("start");
        assert_eq!(start(18), 0, "old, prompt and answer fit");
        assert_eq!(start(15), 4, "prompt and answer fit");
        assert_eq!(start(10), 11, "only the answer fits");
        assert_eq!(
            tail_start(&p, size, 18, 8).expect("start"),
            size - 7,
            "the scan stops at its limit"
        );

        #[derive(Default)]
        struct Collect(Vec<String>);
        impl LineParser for Collect {
            fn consume(&mut self, line: &[u8]) {
                if line.len() <= 10 {
                    self.0.push(String::from_utf8_lossy(line).into_owned());
                }
            }
        }
        let plain = Cursors::new(15, 4, |c: &Collect| c.0.clone());
        assert_eq!(plain.read(&p).expect("read"), vec!["answer"]);
        let free = Cursors::new(15, 4, |c: &Collect| c.0.clone()).long_lines_free(u64::MAX);
        assert_eq!(free.read(&p).expect("read"), vec!["prompt", "answer"]);
        let _ = fs::remove_dir_all(&dir);
    }
}
