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
