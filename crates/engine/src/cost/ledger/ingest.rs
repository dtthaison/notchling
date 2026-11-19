use super::other::{antigravity_call, antigravity_ctx, codex_call, codex_ctx, Reading};
use super::{claude, db, Call, Ledger, Progress, ROW_DAYS};
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

fn read_lines(
    path: &Path,
    offset: u64,
    deadline: Option<Instant>,
    mut each: impl FnMut(&[u8]),
) -> (u64, bool) {
    let Ok(mut file) = File::open(path) else {
        return (offset, true);
    };
    if file.seek(SeekFrom::Start(offset)).is_err() {
        return (offset, true);
    }
    let mut at = offset;
    let mut carry: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = match file.read(&mut buf) {
            Ok(0) | Err(_) => return (at, true),
            Ok(n) => n,
        };
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
            return (at, false);
        }
    }
}

struct Sink<'a> {
    ledger: &'a Ledger,
    cutoff: f64,
    added: u64,
}

impl Sink<'_> {
    fn put(&mut self, found: Option<(u64, Call)>) {
        let Some((key, call)) = found else {
            return;
        };
        if call.ts < self.cutoff {
            return;
        }
        let day = day_key(self.ledger.day_number(call.ts));
        if db::insert(&self.ledger.db, key, &call, &day).unwrap_or(false) {
            self.added += 1;
        }
    }
}

fn update(
    ledger: &Ledger,
    item: &Candidate,
    cutoff: f64,
    deadline: Option<Instant>,
) -> (u64, bool) {
    let text = item.path.to_string_lossy().into_owned();
    let (mut offset, mut state) = match db::mark(&ledger.db, &text) {
        Some((size, _, _)) if size == item.size => return (0, true),
        Some((_, off, _)) if item.size < off => (0, String::new()),
        Some((_, off, state)) => (off, state),
        None => (0, String::new()),
    };
    let id = file_id(&text);
    let catalog = ledger.catalog();
    let mut sink = Sink {
        ledger,
        cutoff,
        added: 0,
    };
    let Ok(tx) = ledger.db.unchecked_transaction() else {
        return (0, true);
    };
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
            });
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
            });
            state = reading.save();
        }
        _ => {
            let ctx = claude::ctx_of(&item.path);
            (offset, done) = read_lines(&item.path, offset, deadline, |line| {
                if !contains(line, br#""usage":{"#) {
                    return;
                }
                let found = serde_json::from_slice::<Value>(line)
                    .ok()
                    .and_then(|o| claude::parse(&o, &ctx, catalog));
                sink.put(found);
            });
        }
    }
    let added = sink.added;
    let size = if done { item.size } else { 0 };
    let marked = db::set_mark(&ledger.db, &text, size, offset, &state);
    if marked.is_err() || tx.commit().is_err() {
        return (0, true);
    }
    (added, done)
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
            progress.left = items.len() - i;
            break;
        }
        let (added, done) = update(ledger, item, cutoff, deadline);
        progress.added += added;
        if !done {
            progress.left = items.len() - i;
            break;
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
