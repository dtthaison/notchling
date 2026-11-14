use super::account_log::{self, AccountSwitches, Entry};
use super::cache::{cell_key, cell_parts, price_key, tool_of, Cache, Cell, Mark, Tokens};
use super::other::{antigravity, codex, OtherPrices};
pub use super::paths::hash;
use super::paths::{account_in_path, contains, file_id, project_name, walk};
use super::prices;
use super::summary::{day_key, owners, summarize, Summary, Tables, Window};
use crate::catalog::{Catalog, Price, Tool};
use crate::pages::clock::local_offset_secs;
use crate::transcript::codex::fork as codex_fork;
use crate::transcript::util::parse_date;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const CLAUDE_CODE: &str = "Claude Code";
pub const DESKTOP: &str = "Desktop";
pub const CODEX: &str = "Codex";
pub const ANTIGRAVITY: &str = "Antigravity CLI";

const KEEP_DAYS: i64 = 400;
const PERSIST_EVERY: f64 = 5.0 * 60.0;
pub(super) const MAX_DEPTH: usize = 32;
const CHUNK: usize = 8 << 20;

#[derive(Clone, Debug)]
pub struct Root {
    pub dir: PathBuf,
    pub channel: &'static str,
}

pub type ZoneFn = fn(f64) -> i64;

#[derive(Default)]
struct Reading {
    model: Option<String>,
    project: Option<String>,
    account: Option<String>,
    total: Option<f64>,
}

struct Ctx<'a> {
    channel: &'static str,
    project: String,
    account: Option<String>,
    file_id: &'a str,
}

struct Request<'a> {
    day: String,
    key: u64,
    account: String,
    channel: &'static str,
    model: String,
    project: String,
    tokens: Tokens,
    price: &'a dyn Fn(&Price) -> f64,
}

struct Recount {
    changed: HashMap<String, (Option<Option<Price>>, Option<Price>)>,
    seen: HashSet<u64>,
    covered: HashMap<(String, String), Covered>,
}

#[derive(Clone, Copy, Default)]
struct Covered {
    old: f64,
    new: f64,
    tokens: Tokens,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ranged {
    pub overall: BTreeMap<Window, Summary>,
    pub by_account: BTreeMap<Window, BTreeMap<String, Summary>>,
}

pub struct Tracker {
    roots: Vec<Root>,
    cache_data: Option<PathBuf>,
    account_log: Option<PathBuf>,
    switches: Option<Arc<dyn AccountSwitches>>,
    zone: ZoneFn,
    catalog: Arc<Catalog>,
    cache: Option<Cache>,
    seen: HashSet<u64>,
    log: Vec<Entry>,
    unsaved: bool,
    last_persist: Option<f64>,
    recount: Option<Recount>,
}

impl Tracker {
    pub fn new(
        roots: Vec<Root>,
        cache_data: Option<PathBuf>,
        account_log: Option<PathBuf>,
        catalog: Arc<Catalog>,
    ) -> Tracker {
        Tracker {
            roots,
            cache_data,
            account_log,
            switches: None,
            zone: local_offset_secs,
            catalog,
            cache: None,
            seen: HashSet::new(),
            log: Vec::new(),
            unsaved: false,
            last_persist: None,
            recount: None,
        }
    }

    pub fn with_switches(mut self, switches: Arc<dyn AccountSwitches>) -> Tracker {
        self.switches = Some(switches);
        self
    }

    pub fn with_zone(mut self, zone: ZoneFn) -> Tracker {
        self.zone = zone;
        self
    }

    fn load(&mut self) {
        if self.cache.is_some() {
            return;
        }
        let cache = match self.cache_data.as_deref() {
            Some(data) => Cache::load(data, &self.catalog),
            None => Cache::empty(),
        };
        self.seen = cache.seen_set();
        self.cache = Some(cache);
        if self.reprice() {
            self.unsaved = true;
        }
    }

    pub fn day_number(&self, ts: f64) -> i64 {
        (ts.floor() as i64 + (self.zone)(ts)).div_euclid(86_400)
    }

    fn day_of(&self, ts: f64) -> String {
        day_key(self.day_number(ts))
    }

    pub fn refresh(&mut self, now: f64) -> bool {
        let file = self
            .account_log
            .as_deref()
            .map(account_log::load)
            .unwrap_or_default();
        self.log = match &self.switches {
            Some(switches) => account_log::merge(file, switches.switches()),
            None => file,
        };
        self.load();
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

    pub fn ranged(&self, now: f64) -> Ranged {
        let Some(cache) = &self.cache else {
            return Ranged::default();
        };
        let tables = Tables::from(cache);
        let today = self.day_number(now);
        let ids = owners(&tables);
        let mut out = Ranged::default();
        for window in Window::ALL {
            out.overall.insert(
                window,
                summarize(&tables, None, window, today, &self.catalog),
            );
            out.by_account.insert(
                window,
                ids.iter()
                    .map(|id| {
                        let s = summarize(&tables, Some(id), window, today, &self.catalog);
                        (id.clone(), s)
                    })
                    .collect(),
            );
        }
        out
    }

    fn reprice(&mut self) -> bool {
        let Some(cache) = self.cache.as_ref() else {
            return false;
        };
        let mut changed = HashMap::new();
        let mut checked = HashSet::new();
        for cells in cache.cells.values() {
            for k in cells.keys() {
                let (_, channel, model, _) = cell_parts(k);
                let tool = tool_of(channel);
                let pk = price_key(tool, model);
                if !checked.insert(pk.clone()) {
                    continue;
                }
                let now = self.catalog.price(tool, model);
                let then = cache.priced_at.get(&pk).copied();
                if then != Some(now) {
                    changed.insert(pk, (then, now));
                }
            }
        }
        if changed.is_empty() {
            return false;
        }
        let tools: HashSet<String> = changed
            .keys()
            .filter_map(|pk| pk.split('\t').next())
            .map(str::to_string)
            .collect();
        self.recount = Some(Recount {
            changed,
            seen: HashSet::new(),
            covered: HashMap::new(),
        });
        for root in self.roots.clone() {
            if !tools.contains(tool_of(root.channel).name()) {
                continue;
            }
            let mut files = Vec::new();
            walk(&root.dir, 0, &mut files);
            for (path, _) in files {
                let text = path.to_string_lossy().into_owned();
                if !Self::accepts(&text, root.channel) {
                    continue;
                }
                let end = self
                    .cache
                    .as_ref()
                    .and_then(|c| c.files.get(&text))
                    .map_or(0, |m| m.offset);
                if end == 0 {
                    continue;
                }
                let file_id = file_id(&text);
                let ctx = Ctx {
                    channel: root.channel,
                    project: project_name(&path, &root.dir),
                    account: account_in_path(&path),
                    file_id: &file_id,
                };
                self.read(&path, 0, Some(end), &ctx, &mut Reading::default());
            }
        }
        let (Some(recount), Some(cache)) = (self.recount.take(), self.cache.as_mut()) else {
            return false;
        };
        for (day, cells) in cache.cells.iter_mut() {
            let moved: HashSet<(String, String)> = cells
                .iter()
                .filter(|(_, c)| c.moved)
                .map(|(k, _)| {
                    let (account, _, model, _) = cell_parts(k);
                    (account.to_string(), model.to_string())
                })
                .collect();
            for (account, model) in &moved {
                reprice_moved(cells, day, account, model, &recount);
            }
            for (k, cell) in cells.iter_mut() {
                let (account, channel, model, _) = cell_parts(k);
                if moved.contains(&(account.to_string(), model.to_string())) {
                    continue;
                }
                let Some(&(then, now)) = recount.changed.get(&price_key(tool_of(channel), model))
                else {
                    continue;
                };
                let covered = recount
                    .covered
                    .get(&(day.clone(), k.clone()))
                    .copied()
                    .unwrap_or_default();
                cell.cost = repriced(cell, &covered, then, now);
            }
        }
        for (pk, (_, now)) in recount.changed {
            cache.priced_at.insert(pk, now);
        }
        true
    }

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
        if root.channel == CODEX {
            let own = codex_fork::own_start(&text);
            if own > mark.offset {
                self.read(path, 0, Some(1), &ctx, &mut reading);
                mark.offset = own;
            }
        }
        mark.offset = self.read(path, mark.offset, None, &ctx, &mut reading);
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

    fn read(
        &mut self,
        path: &Path,
        offset: u64,
        end: Option<u64>,
        ctx: &Ctx,
        reading: &mut Reading,
    ) -> u64 {
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
                if end.is_some_and(|e| line_start + start as u64 >= e) {
                    return line_start + start as u64;
                }
                let stop = start + rel;
                if stop > start {
                    self.ingest(&carry[start..stop], ctx, reading);
                }
                start = stop + 1;
            }
            line_start += start as u64;
            carry.drain(..start);
        }
        line_start
    }

    fn ingest(&mut self, line: &[u8], ctx: &Ctx, reading: &mut Reading) {
        match ctx.channel {
            CODEX => self.codex_line(line, ctx, reading),
            ANTIGRAVITY => self.antigravity_line(line, ctx, reading),
            _ => self.claude_line(line, ctx),
        }
    }

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
        let request = object
            .get("requestId")
            .and_then(Value::as_str)
            .unwrap_or("");
        let project = object
            .get("cwd")
            .and_then(Value::as_str)
            .and_then(|cwd| Path::new(cwd).file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| ctx.project.clone());
        let account = ctx
            .account
            .clone()
            .unwrap_or_else(|| account_log::account_at(date, &self.log));
        let price = |p: &Price| prices::cost_at(p, usage);
        self.take(Request {
            day: self.day_of(date),
            key: hash(&format!("{id}|{request}")),
            account,
            channel: ctx.channel,
            model: prices::model_key(model),
            project,
            tokens: prices::tokens(usage),
            price: &price,
        });
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
                let counts = codex::counts(&t);
                let price = |p: &Price| OtherPrices::cost_at(p, &counts);
                self.take(Request {
                    day: self.day_of(t.date),
                    key: hash(&format!("codex|{}|{id}", ctx.file_id)),
                    account: reading.account.clone().unwrap_or_default(),
                    channel: CODEX,
                    model: reading.model.clone().unwrap_or_else(|| "codex".into()),
                    project: reading.project.clone().unwrap_or_else(|| "Codex".into()),
                    tokens: five(counts),
                    price: &price,
                });
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
                let counts = antigravity::counts(&t);
                let price = |p: &Price| OtherPrices::cost_at(p, &counts);
                self.take(Request {
                    day: self.day_of(t.date),
                    key: hash(&format!("agy|{}|{}", ctx.file_id, t.step)),
                    account: "agy".into(),
                    channel: ANTIGRAVITY,
                    model: reading
                        .model
                        .clone()
                        .unwrap_or_else(|| "Antigravity".into()),
                    project: "Antigravity CLI".into(),
                    tokens: five(counts),
                    price: &price,
                });
            }
        }
    }

    fn take(&mut self, r: Request) {
        let tool = tool_of(r.channel);
        if let Some(recount) = self.recount.as_mut() {
            recount.add(&self.seen, tool, &r);
            return;
        }
        if !self.first_time(r.key, &r.day) || r.tokens.iter().all(|t| *t == 0.0) {
            return;
        }
        let price = self.catalog.price(tool, &r.model);
        let cost = price.as_ref().map_or(0.0, |p| (r.price)(p));
        let Some(cache) = self.cache.as_mut() else {
            return;
        };
        cache.priced_at.insert(price_key(tool, &r.model), price);
        let cell = cache
            .cells
            .entry(r.day.clone())
            .or_default()
            .entry(cell_key(&r.account, r.channel, &r.model, &r.project))
            .or_default();
        cell.cost += cost;
        for (sum, t) in cell.tokens.iter_mut().zip(r.tokens) {
            *sum += t;
        }
    }
}

impl Recount {
    fn add(&mut self, counted: &HashSet<u64>, tool: Tool, r: &Request) {
        let Some(&(then, now)) = self.changed.get(&price_key(tool, &r.model)) else {
            return;
        };
        if !counted.contains(&r.key) || !self.seen.insert(r.key) {
            return;
        }
        let covered = self
            .covered
            .entry((
                r.day.clone(),
                cell_key(&r.account, r.channel, &r.model, &r.project),
            ))
            .or_default();
        covered.old += then.flatten().as_ref().map_or(0.0, r.price);
        covered.new += now.as_ref().map_or(0.0, r.price);
        for (sum, t) in covered.tokens.iter_mut().zip(r.tokens) {
            *sum += t;
        }
    }
}

fn repriced(
    cell: &Cell,
    covered: &Covered,
    then: Option<Option<Price>>,
    now: Option<Price>,
) -> f64 {
    covered.new + uncovered(cell, covered, then, now).1
}

fn uncovered(
    cell: &Cell,
    covered: &Covered,
    then: Option<Option<Price>>,
    now: Option<Price>,
) -> (Tokens, f64) {
    let mut gone = [0.0; 5];
    for (i, g) in gone.iter_mut().enumerate() {
        *g = (cell.tokens[i] - covered.tokens[i]).max(0.0);
    }
    let Some(now) = now else {
        return (gone, 0.0);
    };
    let rest = match then.flatten() {
        Some(old) => {
            let left = (cell.cost - covered.old).max(0.0);
            let at_old = prices::estimate(&old, &gone);
            if at_old > 0.0 {
                left * prices::estimate(&now, &gone) / at_old
            } else {
                prices::estimate(&now, &gone)
            }
        }
        None => prices::estimate(&now, &gone),
    };
    (gone, rest)
}

fn reprice_moved(
    cells: &mut BTreeMap<String, Cell>,
    day: &str,
    account: &str,
    model: &str,
    recount: &Recount,
) {
    let mine = |k: &str| {
        let (a, _, m, _) = cell_parts(k);
        a == account && m == model
    };
    let keys: Vec<String> = cells.keys().filter(|k| mine(k)).cloned().collect();
    let Some(&(then, now)) = keys.first().and_then(|k| {
        recount
            .changed
            .get(&price_key(tool_of(cell_parts(k).1), model))
    }) else {
        return;
    };
    let mut total = Cell::default();
    for k in &keys {
        let c = cells[k];
        total.cost += c.cost;
        for (sum, t) in total.tokens.iter_mut().zip(c.tokens) {
            *sum += t;
        }
    }
    let found: Vec<(&String, &Covered)> = recount
        .covered
        .iter()
        .filter(|((d, k), _)| d == day && mine(k))
        .map(|((_, k), c)| (k, c))
        .collect();
    let mut all = Covered::default();
    for (_, c) in &found {
        all.old += c.old;
        all.new += c.new;
        for (sum, t) in all.tokens.iter_mut().zip(c.tokens) {
            *sum += t;
        }
    }
    let (gone, rest) = uncovered(&total, &all, then, now);
    let gone_sum: f64 = gone.iter().sum();
    let moved = gone_sum >= 0.5;
    let weight = |c: &Cell| c.tokens.iter().sum::<f64>();
    let weights: f64 = keys.iter().map(|k| weight(&cells[k])).sum();
    for k in &keys {
        let share = if weights > 0.0 {
            weight(&cells[k]) / weights
        } else {
            1.0 / keys.len() as f64
        };
        let cell = cells.get_mut(k).expect("listed");
        if moved {
            cell.cost = rest * share;
            for (t, g) in cell.tokens.iter_mut().zip(gone) {
                *t = g * share;
            }
        } else {
            *cell = Cell::default();
        }
        cell.moved = moved;
    }
    for (k, c) in found {
        let cell = cells.entry(k.clone()).or_default();
        cell.cost += c.new;
        for (sum, t) in cell.tokens.iter_mut().zip(c.tokens) {
            *sum += t;
        }
        cell.moved = moved;
    }
    cells.retain(|k, c| !mine(k) || weight(c) >= 0.5 || c.cost.abs() > 1e-9);
}

fn five(c: [f64; 4]) -> Tokens {
    [c[0], c[1], c[2], c[3], 0.0]
}
