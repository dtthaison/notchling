use super::restart::WINDOW_BYTES;
use super::scan::HEAD_BYTES;
use super::tail::{tail_start_with, MAX_LINE_BYTES};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};

const SCAN_BYTES: u64 = 32_000_000;
const KEPT_LINE_BYTES: usize = 8_000_000;

pub(super) struct Window {
    pub(super) lines: Vec<String>,
    pub(super) whole: bool,
}

fn has(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn carries_a_prompt(head: &[u8]) -> bool {
    if has(head, br#""isSidechain":true"#) {
        return false;
    }
    has(head, br#""type":"queued_command""#)
        || (has(head, br#""type":"user""#)
            && !has(head, b"tool_use_id")
            && !has(head, b"toolUseResult"))
}

pub(super) fn read_window(path: &str) -> io::Result<Window> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let start = tail_start_with(path, size, WINDOW_BYTES, SCAN_BYTES, MAX_LINE_BYTES)?;
    file.seek(SeekFrom::Start(start))?;
    let mut reader = BufReader::with_capacity(1 << 20, file.take(size - start));
    let mut lines = Vec::new();
    let mut raw = Vec::new();
    loop {
        raw.clear();
        if reader.read_until(b'\n', &mut raw)? == 0 {
            break;
        }
        if raw.last() == Some(&b'\n') {
            raw.pop();
        }
        if raw.is_empty() {
            continue;
        }
        let big = raw.len() > MAX_LINE_BYTES;
        if big
            && !(raw.len() <= KEPT_LINE_BYTES
                && carries_a_prompt(&raw[..raw.len().min(HEAD_BYTES)]))
        {
            continue;
        }
        lines.push(String::from_utf8_lossy(&raw).into_owned());
    }
    Ok(Window {
        lines,
        whole: start == 0,
    })
}
