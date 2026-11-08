//! Codex sessions as sidebar rows: the rollout files under `<codex home>/sessions/YYYY/MM/DD/`,
//! newest first, read through the engine's `codex::meta` (id, folder, title) and `turn_open`.

use std::path::{Path, PathBuf};

use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::transcript::codex;

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm), to filter the date folders.
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn numbered_dirs(dir: &Path, digits: usize) -> Vec<(u32, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(u32, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if name.len() != digits || !name.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let path = e.path();
            let number: u32 = name.parse().ok()?;
            path.is_dir().then_some((number, path))
        })
        .collect();
    out.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    out
}

/// The rollout files, newest first, at most `limit`. `within_days` keeps only the date folders
/// that many days old or newer (`None` keeps every folder, for a fixture). `.zst` files are skipped.
pub fn rollouts(
    sessions_dir: &Path,
    within_days: Option<i64>,
    now_ms: i64,
    limit: usize,
) -> Vec<PathBuf> {
    let today = now_ms.div_euclid(86_400_000);
    let mut out = Vec::new();
    'years: for (year, year_dir) in numbered_dirs(sessions_dir, 4) {
        for (month, month_dir) in numbered_dirs(&year_dir, 2) {
            for (day, day_dir) in numbered_dirs(&month_dir, 2) {
                if let Some(days) = within_days {
                    let folder_day = days_from_civil(i64::from(year), month, day);
                    if today - folder_day > days {
                        continue;
                    }
                }
                let Ok(entries) = std::fs::read_dir(&day_dir) else {
                    continue;
                };
                let mut files: Vec<PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        name.starts_with("rollout-") && name.ends_with(".jsonl")
                    })
                    .collect();
                files.sort_by(|a, b| b.cmp(a));
                out.extend(files);
                if out.len() >= limit {
                    break 'years;
                }
            }
        }
    }
    out.truncate(limit);
    out
}

/// A rollout as a sidebar row: `codex:<id>`, titled by the first thing typed, Working while its last
/// turn is open, else Idle. `None` when the file has no session line yet.
pub fn session(path: &Path) -> Option<Agent> {
    let text = path.to_string_lossy();
    let meta = codex::meta(&text)?;
    let working = codex::turn_open(&text);
    let since_ms = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let cwd = meta.cwd.clone();
    Some(Agent {
        id: format!("codex:{}", meta.id),
        name: meta
            .title
            .clone()
            .unwrap_or_else(|| format!("Codex {}", &meta.id[..meta.id.len().min(8)])),
        project: cwd.rsplit('/').next().unwrap_or("").to_owned(),
        group: if working {
            AgentGroup::Working
        } else {
            AgentGroup::Completed
        },
        label: if working { "Working" } else { "Idle" }.to_owned(),
        detail: None,
        since_ms,
        target: AgentTarget::Resume {
            session_id: meta.id,
            cwd: cwd.clone(),
        },
        cwd,
        suggested_reply: None,
        transcript_path: Some(text.into_owned()),
        host: "Codex".to_owned(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);

    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    impl Tmp {
        /// Its own folder per test, since the tests run at the same time.
        fn new() -> Tmp {
            let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let dir = std::env::temp_dir()
                .join(format!("notchling-codex-list-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Tmp(dir)
        }

        fn file(&self, rel: &str) -> PathBuf {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "{}\n").unwrap();
            path
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn civil_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(2026, 11, 8), 20_765);
    }

    #[test]
    fn newest_first_within_days_and_limit() {
        let tmp = Tmp::new();
        let old = tmp.file("2026/10/01/rollout-2026-10-01T08-00-00-old.jsonl");
        let a = tmp.file("2026/11/07/rollout-2026-11-07T08-00-00-a.jsonl");
        let b = tmp.file("2026/11/08/rollout-2026-11-08T09-00-00-b.jsonl");
        let c = tmp.file("2026/11/08/rollout-2026-11-08T08-00-00-c.jsonl");
        tmp.file("2026/11/08/rollout-2026-11-08T07-00-00-z.jsonl.zst");
        tmp.file("2026/11/08/notes.txt");
        tmp.file("junk/2026/11/08/rollout-x.jsonl");
        let now_ms = days_from_civil(2026, 11, 8) * 86_400_000 + 3_600_000;
        assert_eq!(
            rollouts(&tmp.0, Some(14), now_ms, 30),
            [b.clone(), c.clone(), a.clone()]
        );
        assert_eq!(
            rollouts(&tmp.0, None, now_ms, 30),
            [b.clone(), c.clone(), a, old]
        );
        assert_eq!(rollouts(&tmp.0, None, now_ms, 1), [b]);
        assert!(rollouts(&tmp.0.join("missing"), None, now_ms, 30).is_empty());
    }

    #[test]
    fn a_file_without_a_session_line_is_no_row() {
        let tmp = Tmp::new();
        let path = tmp.file("2026/11/08/rollout-2026-11-08T08-00-00-x.jsonl");
        assert_eq!(session(&path), None);
    }
}
