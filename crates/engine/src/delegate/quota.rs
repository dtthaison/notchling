use super::options::Tool;
use crate::providers::usage::{Reached, Standing, UsageReport};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FULL_AT: f64 = 95.0;
pub const MAX_AGE_SECS: f64 = 3.0 * 3600.0;
const FILE_PREFIX: &str = "delegate-quota-";

#[derive(Clone, Debug)]
pub struct Source {
    pub dir: PathBuf,
    pub clock: fn() -> f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub label: String,
    pub percent: f64,
    pub resets_at: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pool {
    pub name: String,
    pub windows: Vec<Window>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quota {
    pub fetched: f64,
    pub signed_in: bool,
    #[serde(default)]
    pub reached: Option<Reached>,
    #[serde(default)]
    pub pools: Vec<Pool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Out {
    pub pool: String,
    pub what: String,
    pub resets_at: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Room,
    Out(Out),
    SignedOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Gemini,
    Argon,
    ThirdParty,
}

fn kind_of(model: &str) -> Option<Kind> {
    let model = model.to_lowercase();
    if model.contains("argon") {
        Some(Kind::Argon)
    } else if model.starts_with("gemini") {
        Some(Kind::Gemini)
    } else if model.starts_with("claude") || model.starts_with("gpt-oss") {
        Some(Kind::ThirdParty)
    } else {
        None
    }
}

fn pool_is(kind: Kind, name: &str) -> bool {
    let name = name.to_lowercase();
    match kind {
        Kind::Argon => name.contains("argon"),
        Kind::Gemini => name.contains("gemini") && !name.contains("argon"),
        Kind::ThirdParty => name.contains("claude") || name.contains("gpt"),
    }
}

pub fn pool_name(tool: Tool, model: &str) -> Option<&'static str> {
    match tool {
        Tool::Codex => Some("Codex"),
        Tool::Agy => kind_of(model).map(|k| match k {
            Kind::Argon => "Gemini 4 Argon",
            Kind::Gemini => "Gemini Models",
            Kind::ThirdParty => "Claude and GPT models",
        }),
    }
}

fn what_of(label: &str) -> String {
    match label {
        "Week" => "week quota used up".into(),
        "5h" => "5-hour quota used up".into(),
        other => format!("{} quota used up", other.to_lowercase()),
    }
}

fn full_window(windows: &[Window], now: f64) -> Option<&Window> {
    windows
        .iter()
        .filter(|w| w.percent >= FULL_AT && w.resets_at.is_none_or(|at| at > now))
        .max_by(|a, b| {
            let reset = |w: &Window| w.resets_at.unwrap_or(f64::MAX);
            reset(a).total_cmp(&reset(b))
        })
}

pub fn verdict(quota: Option<&Quota>, tool: Tool, model: &str, now: f64) -> Verdict {
    let Some(quota) = quota else {
        return Verdict::Room;
    };
    if now - quota.fetched > MAX_AGE_SECS {
        return Verdict::Room;
    }
    if !quota.signed_in {
        return Verdict::SignedOut;
    }
    match tool {
        Tool::Codex => {
            if let Some(reached) = quota.reached {
                return Verdict::Out(Out {
                    pool: "Codex".into(),
                    what: match reached {
                        Reached::Credits => "out of credits".into(),
                        Reached::Limit => "limit reached".into(),
                    },
                    resets_at: None,
                });
            }
            for pool in &quota.pools {
                if let Some(w) = full_window(&pool.windows, now) {
                    return Verdict::Out(Out {
                        pool: "Codex".into(),
                        what: what_of(&w.label),
                        resets_at: w.resets_at,
                    });
                }
            }
            Verdict::Room
        }
        Tool::Agy => {
            let Some(kind) = kind_of(model) else {
                return Verdict::Room;
            };
            quota
                .pools
                .iter()
                .filter(|p| pool_is(kind, &p.name))
                .find_map(|p| {
                    full_window(&p.windows, now).map(|w| Out {
                        pool: p.name.clone(),
                        what: what_of(&w.label),
                        resets_at: w.resets_at,
                    })
                })
                .map_or(Verdict::Room, Verdict::Out)
        }
    }
}

pub fn until(resets_at: f64, now: f64) -> String {
    let secs = (resets_at - now).max(0.0) as u64;
    let (days, hours, minutes) = (secs / 86_400, secs % 86_400 / 3_600, secs % 3_600 / 60);
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        "under a minute".into()
    }
}

impl Out {
    pub fn reason(&self, now: f64) -> String {
        match self.resets_at {
            Some(at) if at > now => format!("{}, resets in {}", self.what, until(at, now)),
            _ => self.what.clone(),
        }
    }
}

pub fn note(model: &str, out: &Out, now: f64) -> String {
    format!("{model}: {}", out.reason(now))
}

pub fn refusal(tool: Tool, model: Option<&str>, out: &Out, now: f64) -> String {
    let who = model.unwrap_or(tool.title());
    format!(
        "{who} can't run now. {}: {}. Pick a model from another pool; `notchling delegate-models --for <role>` names the ones with room.",
        out.pool,
        out.reason(now)
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct PoolReady {
    pub name: String,
    pub usable: bool,
    pub reason: Option<&'static str>,
    pub windows: Vec<Window>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ready {
    pub usable: bool,
    pub reason: Option<&'static str>,
    pub fetched: Option<f64>,
    pub age_secs: Option<f64>,
    pub pools: Vec<PoolReady>,
}

fn pool_ready(pool: &Pool, whole: Option<&'static str>, now: f64) -> PoolReady {
    let reason = whole.or_else(|| full_window(&pool.windows, now).map(|_| "pool_full"));
    PoolReady {
        name: pool.name.clone(),
        usable: reason.is_none(),
        reason,
        windows: pool.windows.clone(),
    }
}

pub fn readiness(quota: Option<&Quota>, tool: Tool, now: f64) -> Ready {
    let Some(quota) = quota else {
        return Ready {
            usable: true,
            reason: None,
            fetched: None,
            age_secs: None,
            pools: Vec::new(),
        };
    };
    let whole = if now - quota.fetched > MAX_AGE_SECS {
        Some("stale")
    } else if !quota.signed_in {
        Some("signed_out")
    } else {
        match quota.reached {
            Some(Reached::Credits) => Some("credits_out"),
            Some(Reached::Limit) => Some("limit_reached"),
            None => None,
        }
    };
    let pools: Vec<PoolReady> = quota
        .pools
        .iter()
        .map(|p| pool_ready(p, whole, now))
        .collect();
    let reason = whole.or_else(|| match tool {
        Tool::Codex => pools.iter().find_map(|p| p.reason),
        Tool::Agy => (!pools.is_empty() && pools.iter().all(|p| !p.usable)).then_some("pool_full"),
    });
    Ready {
        usable: reason.is_none(),
        reason,
        fetched: Some(quota.fetched),
        age_secs: Some((now - quota.fetched).max(0.0)),
        pools,
    }
}

pub fn readiness_of_windows(
    fetched: Option<f64>,
    signed_in: bool,
    windows: Vec<Window>,
    now: f64,
) -> Ready {
    let Some(fetched) = fetched else {
        let mut ready = readiness(None, Tool::Agy, now);
        if !signed_in {
            ready.usable = false;
            ready.reason = Some("signed_out");
        }
        return ready;
    };
    let quota = Quota {
        fetched,
        signed_in,
        reached: None,
        pools: vec![Pool {
            name: "Claude".into(),
            windows,
        }],
    };
    readiness(Some(&quota), Tool::Codex, now)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Detected {
    pub label: String,
    pub resets_in: Option<f64>,
}

const SAID_OUT: [&str; 4] = [
    "quota reached",
    "quota exceeded",
    "out of quota",
    "quota has been reached",
];

const MAX_SAID: usize = 1500;

fn parse_wait(after: &str) -> Option<f64> {
    let mut secs = 0.0;
    let mut any = false;
    let mut rest = after.trim_start();
    loop {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            break;
        }
        let tail = &rest[digits.len()..];
        let Some(unit) = tail.chars().next() else {
            break;
        };
        let per = match unit {
            'd' => 86_400.0,
            'h' => 3_600.0,
            'm' => 60.0,
            's' => 1.0,
            _ => break,
        };
        secs += digits.parse::<f64>().ok()? * per;
        any = true;
        rest = tail[unit.len_utf8()..].trim_start();
    }
    any.then_some(secs)
}

pub fn detect(text: &str) -> Option<Detected> {
    if text.len() > MAX_SAID {
        return None;
    }
    text.lines().find_map(|line| {
        let lower = line.to_lowercase();
        let said = SAID_OUT.iter().any(|w| lower.contains(w))
            || (lower.contains("quota") && lower.contains("upgrade your subscription"));
        if !said {
            return None;
        }
        let at = lower.find("quota")?;
        let label = lower[..at]
            .split(|c: char| !c.is_alphabetic())
            .rfind(|w| !w.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "usage".into());
        let resets_in = lower
            .find("resets in ")
            .and_then(|i| parse_wait(&lower[i + "resets in ".len()..]));
        Some(Detected { label, resets_in })
    })
}

pub fn out_of(tool: Tool, model: Option<&str>, found: &Detected, now: f64) -> Out {
    Out {
        pool: model
            .and_then(|m| pool_name(tool, m))
            .unwrap_or(tool.title())
            .to_string(),
        what: what_of(&found.label),
        resets_at: found.resets_in.map(|secs| now + secs),
    }
}

pub fn mark_out(source: &Source, tool: Tool, model: Option<&str>, found: &Detected) {
    let Some(pool) = model.and_then(|m| pool_name(tool, m)) else {
        return;
    };
    let now = source.now();
    let window = Window {
        label: found.label.clone(),
        percent: 100.0,
        resets_at: found.resets_in.map(|secs| now + secs),
    };
    let mut quota = source.load(tool).unwrap_or(Quota {
        fetched: now,
        signed_in: true,
        reached: None,
        pools: Vec::new(),
    });
    quota.fetched = now;
    quota.signed_in = true;
    match quota.pools.iter_mut().find(|p| p.name == pool) {
        Some(existing) => {
            existing.windows.retain(|w| w.label != window.label);
            existing.windows.push(window);
        }
        None => quota.pools.push(Pool {
            name: pool.to_string(),
            windows: vec![window],
        }),
    }
    save(&source.dir, provider_of(tool), &quota);
}

pub fn file_of(dir: &Path, provider: &str) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{provider}.json"))
}

fn provider_of(tool: Tool) -> &'static str {
    match tool {
        Tool::Codex => "codex",
        Tool::Agy => "antigravity",
    }
}

impl Quota {
    pub fn of(
        fetched: f64,
        signed_in: bool,
        report: Option<&UsageReport>,
        standing: Option<&Standing>,
    ) -> Quota {
        let pools = report
            .map(|r| {
                r.groups
                    .iter()
                    .map(|g| Pool {
                        name: g.name.clone().unwrap_or_default(),
                        windows: g
                            .bars
                            .iter()
                            .map(|b| Window {
                                label: b.label.clone(),
                                percent: b.percent,
                                resets_at: b.resets_at,
                            })
                            .collect(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Quota {
            fetched,
            signed_in,
            reached: standing.and_then(|s| s.reached),
            pools,
        }
    }
}

pub fn save(dir: &Path, provider: &str, quota: &Quota) {
    let Ok(text) = serde_json::to_vec(quota) else {
        return;
    };
    let _ = std::fs::create_dir_all(dir);
    let _ = notchling_platform::files::write_private(&file_of(dir, provider), &text);
}

pub fn clear(dir: &Path, provider: &str) {
    let _ = std::fs::remove_file(file_of(dir, provider));
}

pub fn load(dir: &Path, tool: Tool) -> Option<Quota> {
    use std::io::Read;
    let file = std::fs::File::open(file_of(dir, provider_of(tool))).ok()?;
    let mut data = Vec::new();
    file.take(1024 * 1024).read_to_end(&mut data).ok()?;
    serde_json::from_slice(&data).ok()
}

impl Source {
    pub fn now(&self) -> f64 {
        (self.clock)()
    }

    pub fn load(&self, tool: Tool) -> Option<Quota> {
        load(&self.dir, tool)
    }
}

#[cfg(test)]
#[path = "quota_tests.rs"]
mod tests;
