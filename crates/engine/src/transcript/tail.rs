//! Bounded transcript reading: the last few MB of a file as whole lines, and incremental cursors that
//! parse only what was appended since the last read.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Mutex;
use std::time::SystemTime;

/// Transcripts are read from the last 2 MB (the notch) or 4 MB (Chat), never whole.
pub const TAIL_BYTES: u64 = 2_000_000;
pub const CHAT_TAIL_BYTES: u64 = 4_000_000;
/// A line this long is a screenshot or huge tool output: slow to decode and nothing the chat shows.
pub const MAX_LINE_BYTES: usize = 100_000;

/// The complete lines in `start..end` and how many bytes they span. `skip_first_line`: `start` may be
/// mid-line, so everything up to the first newline is dropped. A last line with no newline yet is not
/// returned (its writer is still adding to it).
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

/// The last `max_bytes` of the file as lines (empty lines left out), starting at a line boundary: when
/// the read starts mid-file the first, partial line is dropped. A line that isn't valid UTF-8 is read
/// with the bad bytes replaced.
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

/// Where a tail read should start so that its `budget` bytes hold lines of at most [`MAX_LINE_BYTES`]:
/// longer lines (base64 images) are still read but don't use the budget, so a few of them can't push a
/// session's first prompt out of the read. Always a line start (0 or just past a newline). Looks back at
/// most `scan_limit` bytes and then starts at the oldest whole line it reached.
pub fn tail_start(path: &str, size: u64, budget: u64, scan_limit: u64) -> io::Result<u64> {
    const BLOCK: u64 = 1 << 20;
    let after = |line_end: u64| if line_end >= size { size } else { line_end + 1 };
    let mut file = File::open(path)?;
    let floor = size.saturating_sub(scan_limit);
    let mut kept = 0u64;
    // Where the line being measured ends: its newline's offset, or `size` for the last line.
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
            if length > 0 && length <= MAX_LINE_BYTES as u64 {
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
    // The file's first line, [0, line_end).
    if line_end > MAX_LINE_BYTES as u64 || kept + line_end < budget {
        Ok(0)
    } else {
        Ok(after(line_end))
    }
}

/// A parser that takes a transcript one JSONL line at a time and keeps its own state between lines.
pub trait LineParser: Default {
    fn consume(&mut self, line: &[u8]);
}

struct Cursor<P, O> {
    inode: u64,
    /// Just past the last complete line parsed; a line still being written is read next time.
    offset: u64,
    modified: SystemTime,
    size: u64,
    parser: P,
    output: O,
    /// The clock at the last read, to find the cursor read longest ago.
    used: u64,
}

struct State<P, O> {
    cursors: HashMap<String, Cursor<P, O>>,
    clock: u64,
}

/// Reads transcripts incrementally: each file keeps a cursor (where it was read up to, and the
/// parser's state there), so a call parses only the lines appended since the last one. The first
/// call, and a file that was replaced, truncated or grew by more than `tail_bytes` at once, starts from
/// the last `tail_bytes` of the file. At most `limit` files are kept; reading one more drops the one
/// read longest ago.
pub struct Cursors<P: LineParser, O: Clone> {
    tail_bytes: u64,
    limit: usize,
    /// Above 0, lines over [`MAX_LINE_BYTES`] don't count toward `tail_bytes` ([`tail_start`]).
    long_line_scan: u64,
    render: Box<dyn Fn(&P) -> O + Send + Sync>,
    state: Mutex<State<P, O>>,
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
    /// `render` turns the parser's state into what callers get; it runs only when the file changed.
    pub fn new(
        tail_bytes: u64,
        limit: usize,
        render: impl Fn(&P) -> O + Send + Sync + 'static,
    ) -> Self {
        Cursors {
            tail_bytes,
            limit,
            long_line_scan: 0,
            render: Box::new(render),
            state: Mutex::new(State {
                cursors: HashMap::new(),
                clock: 0,
            }),
        }
    }

    /// Lines over [`MAX_LINE_BYTES`] are read but don't use the tail budget, looking back at most
    /// `scan_limit` bytes: for transcripts whose huge lines hold nothing the reader shows.
    pub fn long_lines_free(mut self, scan_limit: u64) -> Self {
        self.long_line_scan = scan_limit;
        self
    }

    /// None when the file can't be read.
    pub fn read(&self, path: &str) -> Option<O> {
        let meta = fs::metadata(path).ok()?;
        let modified = meta.modified().ok()?;
        let size = meta.len();
        let inode = inode_of(&meta);

        // Held while a cursor reads and parses, so two callers never advance the same one.
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let state = &mut *guard;
        state.clock += 1;
        let clock = state.clock;

        let reusable = match state.cursors.get(path) {
            Some(c)
                if c.inode == inode && c.offset <= size && size - c.offset <= self.tail_bytes =>
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
                    if let Ok((lines, consumed)) = read_chunk(path, cursor.offset, size, false) {
                        cursor.offset += consumed;
                        for line in &lines {
                            cursor.parser.consume(line);
                        }
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
            .then(|| tail_start(path, size, self.tail_bytes, self.long_line_scan).ok())
            .flatten();
        let (start, cut) = match aligned {
            Some(start) => (start, false),
            None => {
                let start = size.saturating_sub(self.tail_bytes);
                (start, start > 0)
            }
        };
        let (lines, consumed) = read_chunk(path, start, size, cut).ok()?;
        let mut parser = P::default();
        for line in &lines {
            parser.consume(line);
        }
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
        // A prompt, three image-sized lines, then a short answer: a Codex image turn.
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
