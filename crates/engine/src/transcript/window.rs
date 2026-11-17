use super::restart::WINDOW_BYTES;
use super::tail::MAX_LINE_BYTES;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

const SCAN_BYTES: u64 = 32_000_000;
const KEPT_LINE_BYTES: u64 = 8_000_000;
const KEPT_TOTAL_BYTES: u64 = 24_000_000;
const HEAD_BYTES: usize = 8_192;
const BLOCK: usize = 1 << 20;
const CACHED_BYTES: usize = 6_000_000;
const CACHED_FILES: usize = 8;

pub(super) struct Window {
    pub(super) lines: Vec<String>,
    pub(super) whole: bool,
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn has(hay: &[u8], needle: &[u8]) -> bool {
    find(hay, needle).is_some()
}

fn leading_type(head: &[u8]) -> Option<&[u8]> {
    let rest = head.strip_prefix(br#"{"type":""#)?;
    let end = rest.iter().position(|&b| b == b'"')?;
    Some(&rest[..end])
}

fn opens_with_a_result(head: &[u8]) -> bool {
    let key = br#""content":[{"#;
    let Some(at) = find(head, key) else {
        return false;
    };
    let from = at + key.len();
    let after = &head[from..head.len().min(from + 120)];
    has(after, br#""type":"tool_result""#) || has(after, b"tool_use_id")
}

fn carries_a_prompt(head: &[u8]) -> bool {
    if has(head, br#""isSidechain":true"#) {
        return false;
    }
    match leading_type(head) {
        Some(b"queue-operation") => return true,
        Some(b"user") | None => {}
        Some(_) => return false,
    }
    has(head, br#""attachment":{"type":"queued_command""#)
        || (has(head, br#""type":"user""#)
            && !has(head, br#""toolUseResult":"#)
            && !opens_with_a_result(head))
}

#[derive(Clone, Copy)]
struct Span {
    start: u64,
    len: u64,
}

struct Plan {
    spans: Vec<Span>,
    whole: bool,
}

struct Scan<'a> {
    file: &'a mut File,
    spans: Vec<Span>,
    ordinary: u64,
    kept: u64,
}

impl Scan<'_> {
    fn head(&mut self, span: Span) -> io::Result<Vec<u8>> {
        let want = span.len.min(HEAD_BYTES as u64) as usize;
        let mut head = vec![0u8; want];
        self.file.seek(SeekFrom::Start(span.start))?;
        self.file.read_exact(&mut head)?;
        Ok(head)
    }

    fn take(&mut self, span: Span) -> io::Result<bool> {
        if span.len == 0 {
            return Ok(true);
        }
        if span.len <= MAX_LINE_BYTES as u64 {
            if self.ordinary + span.len + 1 > WINDOW_BYTES {
                return Ok(false);
            }
            self.ordinary += span.len + 1;
            self.spans.push(span);
            return Ok(true);
        }
        let head = self.head(span)?;
        if !carries_a_prompt(&head) {
            return Ok(true);
        }
        if span.len > KEPT_LINE_BYTES || self.kept + span.len > KEPT_TOTAL_BYTES {
            return Ok(false);
        }
        self.kept += span.len;
        self.spans.push(span);
        Ok(true)
    }
}

fn plan(file: &mut File, size: u64) -> io::Result<Plan> {
    let floor = size.saturating_sub(SCAN_BYTES);
    let mut scan = Scan {
        file,
        spans: Vec::new(),
        ordinary: 0,
        kept: 0,
    };
    let mut buf = vec![0u8; BLOCK];
    let mut line_end = size;
    let mut pos = size;
    while pos > floor {
        let from = pos.saturating_sub(BLOCK as u64).max(floor);
        let len = (pos - from) as usize;
        scan.file.seek(SeekFrom::Start(from))?;
        scan.file.read_exact(&mut buf[..len])?;
        let mut upto = len;
        while let Some(k) = buf[..upto].iter().rposition(|&b| b == b'\n') {
            let newline = from + k as u64;
            let span = Span {
                start: newline + 1,
                len: line_end - (newline + 1),
            };
            if !scan.take(span)? {
                return Ok(finish(scan.spans, false));
            }
            line_end = newline;
            upto = k;
        }
        pos = from;
    }
    if floor > 0 {
        return Ok(finish(scan.spans, false));
    }
    let first = Span {
        start: 0,
        len: line_end,
    };
    let whole = scan.take(first)?;
    Ok(finish(scan.spans, whole))
}

fn finish(mut spans: Vec<Span>, whole: bool) -> Plan {
    spans.reverse();
    Plan { spans, whole }
}

fn push_lines(out: &mut Vec<String>, data: &[u8]) {
    for line in data.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
        out.push(String::from_utf8_lossy(line).into_owned());
    }
}

fn read_spans(file: &mut File, spans: &[Span]) -> io::Result<Vec<String>> {
    let mut lines = Vec::new();
    let mut at = 0;
    while at < spans.len() {
        let first = spans[at];
        let mut end = first.start + first.len;
        let mut next = at + 1;
        if first.len <= MAX_LINE_BYTES as u64 {
            while next < spans.len()
                && spans[next].len <= MAX_LINE_BYTES as u64
                && spans[next].start == end + 1
            {
                end = spans[next].start + spans[next].len;
                next += 1;
            }
        }
        let mut data = vec![0u8; (end - first.start) as usize];
        file.seek(SeekFrom::Start(first.start))?;
        file.read_exact(&mut data)?;
        push_lines(&mut lines, &data);
        at = next;
    }
    Ok(lines)
}

type Cache = HashMap<String, (u64, SystemTime, Arc<Window>)>;

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn weight(window: &Window) -> usize {
    window.lines.iter().map(String::len).sum()
}

#[cfg(test)]
static READS: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[cfg(test)]
pub(super) fn reads_of(path: &str) -> usize {
    READS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|p| *p == path)
        .count()
}

pub(super) fn read_window(path: &str) -> io::Result<Arc<Window>> {
    let mut file = File::open(path)?;
    let meta = file.metadata()?;
    let (size, modified) = (meta.len(), meta.modified().ok());
    if let Some(modified) = modified {
        let hit = cache()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(path)
            .filter(|(len, stamp, _)| *len == size && *stamp == modified)
            .map(|(_, _, window)| window.clone());
        if let Some(window) = hit {
            return Ok(window);
        }
    }
    #[cfg(test)]
    READS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(path.to_string());
    let Plan { spans, whole } = plan(&mut file, size)?;
    let window = Arc::new(Window {
        lines: read_spans(&mut file, &spans)?,
        whole,
    });
    if let Some(modified) = modified.filter(|_| weight(&window) <= CACHED_BYTES) {
        let mut cached = cache().lock().unwrap_or_else(|e| e.into_inner());
        if cached.len() >= CACHED_FILES && !cached.contains_key(path) {
            cached.clear();
        }
        cached.insert(path.to_string(), (size, modified, window.clone()));
    }
    Ok(window)
}
