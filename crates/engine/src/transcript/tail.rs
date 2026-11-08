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
            render: Box::new(render),
            state: Mutex::new(State {
                cursors: HashMap::new(),
                clock: 0,
            }),
        }
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

        let start = size.saturating_sub(self.tail_bytes);
        let (lines, consumed) = read_chunk(path, start, size, start > 0).ok()?;
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
}
