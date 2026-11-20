use super::other::{antigravity_call, antigravity_ctx, codex_call, codex_ctx, Reading};
use super::{claude, db, Call, Ledger, LedgerError, Progress, ROW_DAYS};
use crate::cost::paths::{contains, file_id, walk};
use crate::cost::summary::day_key;
use crate::cost::tracker::{Tracker, ANTIGRAVITY, CODEX};
use crate::transcript::codex::fork as codex_fork;
use serde_json::Value;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

const CHUNK: usize = 8 << 20;

struct Candidate {
    path: PathBuf,
    size: u64,
    channel: &'static str,
    modified: f64,
}

fn modified_of(path: &Path) -> f64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0.0, |d| d.as_secs_f64())
}

fn candidates(ledger: &Ledger, since: f64) -> (Vec<Candidate>, HashSet<String>) {
    let mut found = Vec::new();
    let mut present = HashSet::new();
    for root in ledger.roots() {
        let mut files = Vec::new();
        walk(&root.dir, 0, &mut files);
        for (path, size) in files {
            let text = path.to_string_lossy().into_owned();
            if !Tracker::accepts(&text, root.channel) {
                continue;
            }
            present.insert(text);
            let modified = modified_of(&path);
            if modified >= since {
                found.push(Candidate {
                    path,
                    size,
                    channel: root.channel,
                    modified,
                });
            }
        }
    }
    found.sort_by(|a, b| b.modified.total_cmp(&a.modified));
    (found, present)
}

fn first_line(path: &Path) -> Option<Value> {
    let mut line = String::new();
    BufReader::new(File::open(path).ok()?.take(1 << 20))
        .read_line(&mut line)
        .ok()?;
    serde_json::from_str(line.trim()).ok()
}

const HEAD_BYTES: u64 = 4096;
const TAIL_BYTES: u64 = 256;

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(unix)]
fn inode_of(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).map_or(0, |m| m.ino())
}

#[cfg(not(unix))]
fn inode_of(_path: &Path) -> u64 {
    0
}

fn bytes_at(file: &mut File, at: u64, len: u64) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(at)).ok()?;
    let mut bytes = vec![0u8; len as usize];
    file.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

fn fingerprint_with(path: &Path, at: u64, head: u64, tail: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let first = fnv(&bytes_at(&mut file, 0, head)?);
    let last = fnv(&bytes_at(&mut file, at, tail)?);
    Some(format!(
        "{}:{head}:{first:x}:{at}:{tail}:{last:x}",
        inode_of(path)
    ))
}

fn fingerprint(path: &Path, offset: u64) -> String {
    let tail = offset.min(TAIL_BYTES);
    fingerprint_with(path, offset - tail, offset.min(HEAD_BYTES), tail).unwrap_or_default()
}

fn same_file(path: &Path, stored: &str) -> bool {
    let parts: Vec<&str> = stored.split(':').collect();
    let [_, head, _, at, tail, _] = parts[..] else {
        return false;
    };
    let (Ok(head), Ok(at), Ok(tail)) =
        (head.parse::<u64>(), at.parse::<u64>(), tail.parse::<u64>())
    else {
        return false;
    };
    fingerprint_with(path, at, head, tail).is_some_and(|now| now == stored)
}

fn read_lines(
    path: &Path,
    offset: u64,
    deadline: Option<Instant>,
    mut each: impl FnMut(&[u8]),
) -> Result<(u64, bool), LedgerError> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((offset, true)),
        Err(e) => return Err(e.into()),
    };
    file.seek(SeekFrom::Start(offset))?;
    let mut at = offset;
    let mut carry: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Ok((at, true));
        }
        carry.extend_from_slice(&buf[..n]);
        let mut start = 0;
        while let Some(rel) = carry[start..].iter().position(|&b| b == b'\n') {
            let stop = start + rel;
            if stop > start {
                each(&carry[start..stop]);
            }
            start = stop + 1;
        }
        at += start as u64;
        carry.drain(..start);
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return Ok((at, false));
        }
    }
}

struct Sink<'a> {
    ledger: &'a Ledger,
    cutoff: f64,
    added: u64,
    failed: Option<LedgerError>,
}

impl Sink<'_> {
    fn put(&mut self, found: Option<(u64, Call)>) {
        let Some((key, call)) = found else {
            return;
        };
        if call.ts < self.cutoff || self.failed.is_some() {
            return;
        }
        let day = day_key(self.ledger.day_number(call.ts));
        match db::insert(&self.ledger.db, key, &call, &day) {
            Ok(true) => self.added += 1,
            Ok(false) => {}
            Err(e) => self.failed = Some(e.into()),
        }
    }
}

fn update(
    ledger: &Ledger,
    item: &Candidate,
    cutoff: f64,
    deadline: Option<Instant>,
) -> Result<(u64, bool), LedgerError> {
    let text = item.path.to_string_lossy().into_owned();
    let (mut offset, mut state) = match db::mark(&ledger.db, &text) {
        Some(m) if !same_file(&item.path, &m.fp) || item.size < m.size || item.size < m.off => {
            (0, String::new())
        }
        Some(m) if m.size == item.size => return Ok((0, true)),
        Some(m) => (m.off, m.state),
        None => (0, String::new()),
    };
    let id = file_id(&text);
    let catalog = ledger.catalog();
    let mut sink = Sink {
        ledger,
        cutoff,
        added: 0,
        failed: None,
    };
    let tx = ledger.db.unchecked_transaction()?;
    let mut reading = Reading::load(&state);
    let done;
    match item.channel {
        CODEX => {
            let first = first_line(&item.path);
            let ctx = codex_ctx(&item.path, first.as_ref(), &id);
            let own = codex_fork::own_start(&text);
            if own > offset {
                offset = own;
            }
            (offset, done) = read_lines(&item.path, offset, deadline, |line| {
                if !(contains(line, br#""token_count""#) || contains(line, br#""turn_context""#)) {
                    return;
                }
                let found = serde_json::from_slice::<Value>(line)
                    .ok()
                    .and_then(|o| codex_call(&o, &ctx, &mut reading, catalog));
                sink.put(found);
            })?;
            state = reading.save();
        }
        ANTIGRAVITY => {
            let ctx = antigravity_ctx(&item.path, &id);
            (offset, done) = read_lines(&item.path, offset, deadline, |line| {
                if !(contains(line, br#""input_tokens""#) || contains(line, b"Model Selection")) {
                    return;
                }
                let found = serde_json::from_slice::<Value>(line)
                    .ok()
                    .and_then(|o| antigravity_call(&o, &ctx, &mut reading, catalog));
                sink.put(found);
            })?;
            state = reading.save();
        }
        _ => {
            let ctx = claude::ctx_of(&item.path);
            (offset, done) = read_lines(&item.path, offset, deadline, |line| {
                if !contains(line, br#""usage""#) {
                    return;
                }
                let found = serde_json::from_slice::<Value>(line)
                    .ok()
                    .and_then(|o| claude::parse(&o, &ctx, catalog));
                sink.put(found);
            })?;
        }
    }
    if let Some(e) = sink.failed.take() {
        return Err(e);
    }
    let added = sink.added;
    let size = if done { item.size } else { 0 };
    let fp = fingerprint(&item.path, offset);
    db::set_mark(&ledger.db, &text, size, offset, &state, &fp)?;
    tx.commit()?;
    Ok((added, done))
}

pub fn refresh(ledger: &mut Ledger, now: f64, budget: Option<Duration>) -> Progress {
    let started = Instant::now();
    let deadline = budget.map(|b| started + b);
    let cutoff = now - (ROW_DAYS * 86_400) as f64;
    let (items, present) = candidates(ledger, cutoff);
    let mut progress = Progress {
        files: items.len(),
        ..Progress::default()
    };
    for (i, item) in items.iter().enumerate() {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            progress.left += items.len() - i;
            break;
        }
        match update(ledger, item, cutoff, deadline) {
            Ok((added, true)) => progress.added += added,
            Ok((added, false)) => {
                progress.added += added;
                progress.left += items.len() - i;
                break;
            }
            Err(_) => progress.left += 1,
        }
    }
    if progress.complete() {
        for path in db::marked_paths(&ledger.db) {
            if !present.contains(&path) {
                db::forget(&ledger.db, &path);
            }
        }
        db::set_meta(&ledger.db, "scanned", &format!("{now}"));
    }
    ledger.prune(now);
    progress
}
