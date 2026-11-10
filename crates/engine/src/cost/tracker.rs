//! `CostTracker`: what the token use in the transcripts would have cost at API prices, from the
//! `usage` of every assistant message (Claude Code, Claude Desktop), every Codex `token_count` and
//! every Antigravity `PLANNER_RESPONSE`.
//!
//! A month of transcripts can be several GB, so each file is read once and then only from where
//! the last scan stopped. Running totals, read offsets and the ids already counted are kept in a
//! [`Cache`]. It only reads: transcripts and the account log are never written.
//!
//! The scan walks the roots (symlinks are not followed) and reads the files whose size moved. A
//! message is counted once whichever transcript holds it: Claude Code copies messages into the new
//! transcript when it compacts or resumes, so totals are lower than its `/stats`. The key is
//! `message.id|requestId` (Codex: file and ordinal; Antigravity: file and step), hashed with
//! FNV-1a.

use super::account_log::{self, Entry};
use super::cache::{Cache, Mark};
use super::other::{antigravity, codex, OtherPrices};
pub use super::paths::hash;
use super::paths::{account_in_path, contains, file_id, project_name, walk};
use super::prices;
use super::summary::{day_key, key, owners, summarize, Summary, Window};
use crate::pages::clock::local_offset_secs;
use crate::transcript::util::parse_date;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const CLAUDE_CODE: &str = "Claude Code";
pub const DESKTOP: &str = "Desktop";
pub const CODEX: &str = "Codex";
pub const ANTIGRAVITY: &str = "Antigravity CLI";

/// Days are kept this long.
const KEEP_DAYS: i64 = 400;
/// The cache is several MB of JSON in one row, so it is saved at most this often.
const PERSIST_EVERY: f64 = 5.0 * 60.0;
/// Folders deeper than this are not read, which also ends any loop a mount might make.
pub(super) const MAX_DEPTH: usize = 32;
const CHUNK: usize = 8 << 20;

/// A folder of transcripts and the channel its files belong to.
#[derive(Clone, Debug)]
pub struct Root {
    pub dir: PathBuf,
    pub channel: &'static str,
}

/// Seconds the zone is ahead of UTC at a time.
pub type ZoneFn = fn(f64) -> i64;

/// What a file's later lines depend on, kept between reads.
#[derive(Default)]
struct Reading {
    model: Option<String>,
    project: Option<String>,
    account: Option<String>,
    total: Option<f64>,
}

/// Per file: where its lines are counted from.
struct Ctx<'a> {
    channel: &'static str,
    /// The folder name under the root, for a message with no `cwd`.
    project: String,
    /// Desktop transcripts carry the account in their path.
    account: Option<String>,
    file_id: &'a str,
}

/// All the summaries, per window and per account.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ranged {
    pub overall: BTreeMap<Window, Summary>,
    pub by_account: BTreeMap<Window, BTreeMap<String, Summary>>,
}

pub struct Tracker {
    roots: Vec<Root>,
    cache_data: Option<PathBuf>,
    account_log: Option<PathBuf>,
    zone: ZoneFn,
    cache: Option<Cache>,
    seen: HashSet<u64>,
    log: Vec<Entry>,
    unsaved: bool,
    last_persist: Option<f64>,
}

impl Tracker {
    /// `cache_data` is the data folder whose `notchling.db` holds the cache; `None` keeps it in
    /// memory (tests). `account_log` is the account store's
    /// `account-log.json`, read for the CLI's transcripts.
    pub fn new(
        roots: Vec<Root>,
        cache_data: Option<PathBuf>,
        account_log: Option<PathBuf>,
    ) -> Tracker {
        Tracker {
            roots,
            cache_data,
            account_log,
            zone: local_offset_secs,
            cache: None,
            seen: HashSet::new(),
            log: Vec::new(),
            unsaved: false,
            last_persist: None,
        }
    }

    /// A fixed zone for day boundaries (tests).
    pub fn with_zone(mut self, zone: ZoneFn) -> Tracker {
        self.zone = zone;
        self
    }

    fn load(&mut self) {
        if self.cache.is_some() {
            return;
        }
        let cache = self
            .cache_data
            .as_deref()
            .map(Cache::load)
            .unwrap_or_default();
        self.seen = cache.seen_set();
        self.cache = Some(cache);
    }

    /// The day number (days since 1970 in the zone) of a time.
    pub fn day_number(&self, ts: f64) -> i64 {
        (ts.floor() as i64 + (self.zone)(ts)).div_euclid(86_400)
    }

    fn day_of(&self, ts: f64) -> String {
        day_key(self.day_number(ts))
    }

    /// Reads whatever was added to the transcripts since the last scan; true when anything was
    /// counted or an offset moved. Saves the cache if it is due.
    pub fn refresh(&mut self, now: f64) -> bool {
        self.load();
        self.log = self
            .account_log
            .as_deref()
            .map(account_log::load)
            .unwrap_or_default();
        let dirty = self.scan_all();
        self.prune(now);
        if dirty {
            self.unsaved = true;
        }
        if self.unsaved && self.last_persist.is_none_or(|t| now - t >= PERSIST_EVERY) {
            self.persist(now);
        }
        dirty
    }

    /// Writes counts not saved yet.
    pub fn save_now(&mut self, now: f64) {
        if self.unsaved {
            self.persist(now);
        }
    }

    fn persist(&mut self, now: f64) {
        let (Some(data), Some(cache)) = (&self.cache_data, &self.cache) else {
            self.unsaved = false;
            return;
        };
        if cache.save(data).is_ok() {
            self.unsaved = false;
            self.last_persist = Some(now);
        }
    }

    fn prune(&mut self, now: f64) {
        let cutoff = day_key(self.day_number(now) - KEEP_DAYS);
        let Some(cache) = self.cache.as_mut() else {
            return;
        };
        let old: Vec<u64> = cache
            .seen
            .iter()
            .filter(|(d, _)| d.as_str() < cutoff.as_str())
            .flat_map(|(_, h)| h.iter().copied())
            .collect();
        for h in old {
            self.seen.remove(&h);
        }
        cache.prune_before(&cutoff);
    }

    /// Every window's summary, overall and per account, as of `now`.
    pub fn ranged(&self, now: f64) -> Ranged {
        let Some(cache) = &self.cache else {
            return Ranged::default();
        };
        let today = self.day_number(now);
        let ids = owners(cache);
        let mut out = Ranged::default();
        for window in Window::ALL {
            out.overall
                .insert(window, summarize(cache, None, window, today));
            out.by_account.insert(
                window,
                ids.iter()
                    .map(|id| (id.clone(), summarize(cache, Some(id), window, today)))
                    .collect(),
            );
        }
        out
    }

    // MARK: Scanning

    /// Whether a file under a channel's root is a transcript to read: Codex's rollouts (the
    /// compressed `.zst` ones are skipped) and Antigravity's `transcript.jsonl` (not
    /// `transcript_full.jsonl` or the chunk files), or any `.jsonl` for the Claude channels.
    pub fn accepts(path: &str, channel: &str) -> bool {
        if !path.ends_with(".jsonl") {
            return false;
        }
        let name = path.rsplit('/').next().unwrap_or(path);
        match channel {
            CODEX => name.starts_with("rollout-"),
            ANTIGRAVITY => name == "transcript.jsonl",
            _ => true,
        }
    }

    /// Every transcript, however old: "All Time" needs the whole history. Each is read once (by
    /// offset).
    fn scan_all(&mut self) -> bool {
        let mut dirty = false;
        let mut present: HashSet<String> = HashSet::new();
        for root in self.roots.clone() {
            let mut files = Vec::new();
            walk(&root.dir, 0, &mut files);
            for (path, size) in files {
                let text = path.to_string_lossy().into_owned();
                if !Self::accepts(&text, root.channel) {
                    continue;
                }
                present.insert(text);
                if self.update(&path, size, &root) {
                    dirty = true;
                }
            }
        }
        if let Some(cache) = self.cache.as_mut() {
            let gone: Vec<String> = cache
                .files
                .keys()
                .filter(|k| !present.contains(*k))
                .cloned()
                .collect();
            dirty |= !gone.is_empty();
            for k in gone {
                cache.files.remove(&k);
            }
        }
        dirty
    }

    /// Reads what was added to one transcript since its saved offset. True when anything moved.
    fn update(&mut self, path: &Path, size: u64, root: &Root) -> bool {
        let text = path.to_string_lossy().into_owned();
        let mut mark = self
            .cache
            .as_ref()
            .and_then(|c| c.files.get(&text))
            .cloned()
            .unwrap_or_default();
        if mark.size == size {
            return false;
        }
        // A file that shrank was rewritten; read it again (ids already counted are skipped).
        if size < mark.offset {
            mark = Mark::default();
        }
        let mut reading = Reading {
            model: mark.model.clone(),
            project: mark.project.clone(),
            account: mark.account.clone(),
            total: mark.total,
        };
        let file_id = file_id(&text);
        let ctx = Ctx {
            channel: root.channel,
            project: project_name(path, &root.dir),
            account: account_in_path(path),
            file_id: &file_id,
        };
        mark.offset = self.read(path, mark.offset, &ctx, &mut reading);
        mark.size = size;
        mark.model = reading.model;
        mark.project = reading.project;
        mark.account = reading.account;
        mark.total = reading.total;
        if let Some(cache) = self.cache.as_mut() {
            cache.files.insert(text, mark);
        }
        true
    }

    /// Counts every complete line from `offset` on and returns where the last complete line ends.
    fn read(&mut self, path: &Path, offset: u64, ctx: &Ctx, reading: &mut Reading) -> u64 {
        let Ok(mut file) = File::open(path) else {
            return offset;
        };
        if file.seek(SeekFrom::Start(offset)).is_err() {
            return offset;
        }
        let mut line_start = offset;
        let mut carry: Vec<u8> = Vec::new();
        let mut buf = vec![0u8; CHUNK];
        loop {
            let n = match file.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            carry.extend_from_slice(&buf[..n]);
            let mut start = 0;
            while let Some(rel) = carry[start..].iter().position(|&b| b == b'\n') {
                let end = start + rel;
                if end > start {
                    self.ingest(&carry[start..end], ctx, reading);
                }
                start = end + 1;
            }
            line_start += start as u64;
            carry.drain(..start);
        }
        line_start
    }

    /// One line of a transcript, by the channel it came from.
    fn ingest(&mut self, line: &[u8], ctx: &Ctx, reading: &mut Reading) {
        match ctx.channel {
            CODEX => self.codex_line(line, ctx, reading),
            ANTIGRAVITY => self.antigravity_line(line, ctx, reading),
            _ => self.claude_line(line, ctx),
        }
    }

    /// True when `key` is new (and now counted on `day`).
    fn first_time(&mut self, key: u64, day: &str) -> bool {
        if !self.seen.insert(key) {
            return false;
        }
        if let Some(cache) = self.cache.as_mut() {
            cache.seen.entry(day.to_string()).or_default().push(key);
        }
        true
    }

    fn claude_line(&mut self, line: &[u8], ctx: &Ctx) {
        if !contains(line, br#""usage":{"#) {
            return;
        }
        let Ok(object) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        if object.get("type").and_then(Value::as_str) != Some("assistant") {
            return;
        }
        let Some(message) = object.get("message") else {
            return;
        };
        let (Some(model), Some(usage), Some(id), Some(date)) = (
            message.get("model").and_then(Value::as_str),
            message.get("usage").filter(|u| u.is_object()),
            message.get("id").and_then(Value::as_str),
            object
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(parse_date),
        ) else {
            return;
        };
        if model == "<synthetic>" {
            return;
        }
        let day = self.day_of(date);
        let request = object
            .get("requestId")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !self.first_time(hash(&format!("{id}|{request}")), &day) {
            return;
        }
        let cost = prices::cost(model, usage);
        if cost <= 0.0 {
            return;
        }
        let name = object
            .get("cwd")
            .and_then(Value::as_str)
            .and_then(|cwd| Path::new(cwd).file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| ctx.project.clone());
        // Desktop transcripts carry the account in their path; the CLI's come from the switch log.
        let account = ctx
            .account
            .clone()
            .unwrap_or_else(|| account_log::account_at(date, &self.log));
        self.record(
            &day,
            &account,
            ctx.channel,
            &prices::model_key(model),
            &name,
            cost,
            prices::tokens(usage),
        );
    }

    fn codex_line(&mut self, line: &[u8], ctx: &Ctx, reading: &mut Reading) {
        if !(contains(line, br#""token_count""#)
            || contains(line, br#""turn_context""#)
            || contains(line, br#""session_meta""#))
        {
            return;
        }
        let Some(event) = serde_json::from_slice::<Value>(line)
            .ok()
            .and_then(|o| codex::parse(&o))
        else {
            return;
        };
        match event {
            codex::Event::Session { account } => {
                if let Some(account) = account {
                    reading.account = Some(account);
                }
            }
            codex::Event::Turn { model, folder } => {
                if let Some(model) = model {
                    reading.model = Some(model);
                }
                if let Some(folder) = folder {
                    reading.project = Path::new(&folder)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned());
                }
            }
            codex::Event::Tokens(t) => {
                // Codex repeats an event now and then; a real request always moves the running
                // total.
                if t.total >= 0.0 {
                    if reading.total == Some(t.total) {
                        return;
                    }
                    reading.total = Some(t.total);
                }
                let id = match t.ordinal {
                    Some(o) => format!("{o}"),
                    None => format!("{}", t.date),
                };
                let day = self.day_of(t.date);
                if !self.first_time(hash(&format!("codex|{}|{id}", ctx.file_id)), &day) {
                    return;
                }
                let model = reading.model.clone().unwrap_or_else(|| "codex".into());
                let counts = codex::counts(&t);
                let project = reading.project.clone().unwrap_or_else(|| "Codex".into());
                let account = reading.account.clone().unwrap_or_default();
                self.record(
                    &day,
                    &account,
                    CODEX,
                    &model,
                    &project,
                    OtherPrices::cost(&model, &counts),
                    counts,
                );
            }
        }
    }

    fn antigravity_line(&mut self, line: &[u8], ctx: &Ctx, reading: &mut Reading) {
        if !(contains(line, br#""input_tokens""#) || contains(line, b"Model Selection")) {
            return;
        }
        let Some(event) = serde_json::from_slice::<Value>(line)
            .ok()
            .and_then(|o| antigravity::parse(&o))
        else {
            return;
        };
        match event {
            antigravity::Event::Model(name) => reading.model = Some(name),
            antigravity::Event::Tokens(t) => {
                let day = self.day_of(t.date);
                if !self.first_time(hash(&format!("agy|{}|{}", ctx.file_id, t.step)), &day) {
                    return;
                }
                let model = reading
                    .model
                    .clone()
                    .unwrap_or_else(|| "Antigravity".into());
                let counts = antigravity::counts(&t);
                self.record(
                    &day,
                    "agy",
                    ANTIGRAVITY,
                    &model,
                    "Antigravity CLI",
                    OtherPrices::cost(&model, &counts),
                    counts,
                );
            }
        }
    }

    /// Adds one request to the day's totals. A cost of 0 still counts its tokens (a model without
    /// a price).
    #[allow(clippy::too_many_arguments)]
    fn record(
        &mut self,
        day: &str,
        account: &str,
        channel: &str,
        model: &str,
        project: &str,
        cost: f64,
        counts: [f64; 4],
    ) {
        let Some(cache) = self.cache.as_mut() else {
            return;
        };
        let model_key = key(account, model);
        let slot = |table: &mut BTreeMap<String, BTreeMap<String, f64>>, k: String| {
            *table
                .entry(day.to_string())
                .or_default()
                .entry(k)
                .or_insert(0.0) += cost;
        };
        slot(&mut cache.models, model_key.clone());
        slot(&mut cache.projects, key(account, project));
        slot(&mut cache.channels, key(account, channel));
        *cache
            .channel_tokens
            .entry(day.to_string())
            .or_default()
            .entry(key(account, channel))
            .or_insert(0.0) += counts.iter().sum::<f64>();
        let sum = cache
            .tokens
            .entry(day.to_string())
            .or_default()
            .entry(model_key)
            .or_insert([0.0; 4]);
        for i in 0..4 {
            sum[i] += counts[i];
        }
    }
}
