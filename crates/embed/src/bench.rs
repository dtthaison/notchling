use crate::granite::Granite;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub load_ms: f64,
    pub query_p50_ms: f64,
    pub query_p95_ms: f64,
    pub chunks_per_s: f64,
    pub chunks_per_s_at_budget: f64,
    pub rss_mb: f64,
    pub peak_rss_mb: f64,
    pub peak_footprint_mb: f64,
}

pub const ROUNDS: usize = 3;
pub const CHUNKS: usize = 64;
pub const CHUNK_CHARS: usize = 1200;
pub const BATCH: usize = 8;

fn read_set(data: &Path, set: &str) -> Result<Value, String> {
    let file = data.join(format!("{set}.json"));
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))
}

pub fn queries(data: &Path) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for set in ["settings", "chat"] {
        let v = read_set(data, set)?;
        let list = v["queries"]
            .as_array()
            .ok_or(format!("{set}.json has no queries"))?;
        for q in list {
            let text = q.get(0).and_then(Value::as_str).or_else(|| q.as_str());
            out.push(
                text.ok_or(format!("{set}.json has a query that is not text"))?
                    .to_string(),
            );
        }
    }
    Ok(out)
}

pub fn chunks(data: &Path, n: usize, chars: usize) -> Result<Vec<String>, String> {
    let v = read_set(data, "chat")?;
    let mut docs: Vec<(&String, &str)> = v["docs"]
        .as_object()
        .ok_or("chat.json has no docs")?
        .iter()
        .filter_map(|(k, d)| d.as_str().map(|d| (k, d)))
        .collect();
    docs.sort();
    if docs.is_empty() {
        return Err("chat.json has no docs".into());
    }
    let mut out = Vec::with_capacity(n);
    let mut i = 0;
    while out.len() < n {
        let mut s = String::new();
        while s.chars().count() < chars {
            s.push_str(docs[i % docs.len()].1);
            s.push('\n');
            i += 1;
        }
        out.push(s.chars().take(chars).collect());
    }
    Ok(out)
}

fn percentile(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}

pub fn limit_threads(n: usize) {
    let n = n.max(1).to_string();
    std::env::set_var("RAYON_NUM_THREADS", &n);
    std::env::set_var("VECLIB_MAXIMUM_THREADS", &n);
}

pub fn run(dir: &Path, data: &Path, budget: f64) -> Result<Report, String> {
    let budget = budget.clamp(0.01, 1.0);
    let qs = queries(data)?;
    let cs = chunks(data, CHUNKS, CHUNK_CHARS)?;
    let start = Instant::now();
    let g = Granite::load(dir)?;
    let load_ms = start.elapsed().as_secs_f64() * 1000.0;
    for q in qs.iter().take(3) {
        g.embed(&[q.as_str()])?;
    }
    let mut lat = Vec::with_capacity(qs.len() * ROUNDS);
    for _ in 0..ROUNDS {
        for q in &qs {
            let t = Instant::now();
            g.embed(&[q.as_str()])?;
            lat.push(t.elapsed().as_secs_f64() * 1000.0);
        }
    }
    let refs: Vec<&str> = cs.iter().map(String::as_str).collect();
    let t = Instant::now();
    for batch in refs.chunks(BATCH) {
        g.embed(batch)?;
    }
    let chunks_per_s = cs.len() as f64 / t.elapsed().as_secs_f64();
    let t = Instant::now();
    for batch in refs.chunks(BATCH) {
        let b = Instant::now();
        g.embed(batch)?;
        let busy = b.elapsed().as_secs_f64();
        std::thread::sleep(Duration::from_secs_f64(busy * (1.0 - budget) / budget));
    }
    let chunks_per_s_at_budget = cs.len() as f64 / t.elapsed().as_secs_f64();
    Ok(Report {
        load_ms,
        query_p50_ms: percentile(&mut lat.clone(), 0.5),
        query_p95_ms: percentile(&mut lat, 0.95),
        chunks_per_s,
        chunks_per_s_at_budget,
        rss_mb: rss_mb(),
        peak_rss_mb: peak_rss_mb(),
        peak_footprint_mb: peak_footprint_mb(),
    })
}

#[cfg(target_os = "macos")]
fn usage() -> Option<libc::rusage_info_v4> {
    let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    let ptr = &mut info as *mut libc::rusage_info_v4 as *mut libc::rusage_info_t;
    let rc = unsafe {
        libc::proc_pid_rusage(std::process::id() as libc::c_int, libc::RUSAGE_INFO_V4, ptr)
    };
    (rc == 0).then_some(info)
}

#[cfg(target_os = "macos")]
pub fn rss_mb() -> f64 {
    usage().map_or(0.0, |u| u.ri_resident_size as f64 / (1024.0 * 1024.0))
}

#[cfg(target_os = "macos")]
pub fn peak_footprint_mb() -> f64 {
    usage().map_or(0.0, |u| {
        u.ri_lifetime_max_phys_footprint as f64 / (1024.0 * 1024.0)
    })
}

#[cfg(not(target_os = "macos"))]
pub fn peak_footprint_mb() -> f64 {
    peak_rss_mb()
}

#[cfg(target_os = "linux")]
pub fn rss_mb() -> f64 {
    let pages = std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| {
            s.split_whitespace()
                .nth(1)
                .and_then(|p| p.parse::<f64>().ok())
        })
        .unwrap_or(0.0);
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as f64;
    pages * page / (1024.0 * 1024.0)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn rss_mb() -> f64 {
    0.0
}

#[cfg(unix)]
pub fn peak_rss_mb() -> f64 {
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) } != 0 {
        return 0.0;
    }
    let unit = if cfg!(target_os = "macos") {
        1.0
    } else {
        1024.0
    };
    ru.ru_maxrss as f64 * unit / (1024.0 * 1024.0)
}

#[cfg(not(unix))]
pub fn peak_rss_mb() -> f64 {
    0.0
}
