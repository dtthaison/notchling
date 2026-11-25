use super::list::{rollout_session, rollouts, rollouts_skipping, MAX_SCANNED};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Tmp(PathBuf);

impl Tmp {
    fn new() -> Tmp {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("nl-codex-skip-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Tmp(dir)
    }

    fn rollout(&self, day: u32, n: usize, id: &str) -> PathBuf {
        let path = self.0.join(format!(
            "2026/11/{day:02}/rollout-2026-11-{day:02}T{:02}-{:02}-00-{id}.jsonl",
            n / 60,
            n % 60
        ));
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

fn skip(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|id| format!("codex:{id}")).collect()
}

const NOW: i64 = 1_794_000_000_000;

#[test]
fn a_skipped_rollout_is_set_aside_by_its_file_name() {
    let tmp = Tmp::new();
    let real = tmp.rollout(10, 1, "real-1");
    let job = tmp.rollout(10, 2, "job-1");
    let got = rollouts_skipping(&tmp.0, None, NOW, 30, &skip(&["job-1"]));
    assert_eq!(got.shown, [real]);
    assert_eq!(got.skipped, [job]);
}

#[test]
fn only_the_whole_id_in_the_name_is_skipped() {
    let tmp = Tmp::new();
    let longer = tmp.rollout(10, 1, "xjob-1");
    let shorter = tmp.rollout(10, 2, "job");
    let job = tmp.rollout(10, 3, "job-1");
    let got = rollouts_skipping(&tmp.0, None, NOW, 30, &skip(&["job-1", "agy:job"]));
    assert_eq!(got.shown, [shorter, longer]);
    assert_eq!(got.skipped, [job]);
}

#[test]
fn the_limit_counts_only_the_rollouts_that_are_not_skipped() {
    let tmp = Tmp::new();
    let jobs: Vec<String> = (0..50).map(|n| format!("job-{n}")).collect();
    for (n, id) in jobs.iter().enumerate() {
        tmp.rollout(14, n, id);
    }
    let old: Vec<PathBuf> = (0..5)
        .map(|n| tmp.rollout(10, n, &format!("real-{n}")))
        .collect();
    let ids: Vec<&str> = jobs.iter().map(String::as_str).collect();
    let got = rollouts_skipping(&tmp.0, None, NOW, 3, &skip(&ids));
    assert_eq!(got.shown, [old[4].clone(), old[3].clone(), old[2].clone()]);
    assert_eq!(got.skipped.len(), 3);
    assert_eq!(rollouts(&tmp.0, None, NOW, 3).len(), 3);
    assert!(rollouts(&tmp.0, None, NOW, 3)
        .iter()
        .all(|p| p.to_string_lossy().contains("-job-")));
}

#[test]
fn the_scan_stops_after_a_bound_even_when_everything_is_skipped() {
    let tmp = Tmp::new();
    let jobs: Vec<String> = (0..MAX_SCANNED + 20).map(|n| format!("job-{n}")).collect();
    for (n, id) in jobs.iter().enumerate() {
        tmp.rollout(14, n, id);
    }
    tmp.rollout(10, 0, "real-0");
    let ids: Vec<&str> = jobs.iter().map(String::as_str).collect();
    let got = rollouts_skipping(&tmp.0, None, NOW, 30, &skip(&ids));
    assert!(got.shown.is_empty(), "{:?}", got.shown);
    assert_eq!(got.skipped.len(), 30);
}

#[test]
fn without_a_skip_set_it_is_the_plain_listing() {
    let tmp = Tmp::new();
    let a = tmp.rollout(10, 1, "a");
    let b = tmp.rollout(11, 1, "b");
    let got = rollouts_skipping(&tmp.0, None, NOW, 30, &HashSet::new());
    assert_eq!(got.shown, [b.clone(), a.clone()]);
    assert!(got.skipped.is_empty());
    assert_eq!(rollouts(&tmp.0, None, NOW, 30), [b, a]);
    assert!(
        rollouts_skipping(Path::new("/nonexistent/x"), None, NOW, 30, &skip(&["a"]))
            .shown
            .is_empty()
    );
}

#[test]
fn a_rollouts_session_id_is_what_follows_its_timestamp() {
    assert_eq!(
        rollout_session("rollout-2026-11-14T10-00-00-019aaaaa-0000.jsonl"),
        Some("019aaaaa-0000")
    );
    assert_eq!(rollout_session("rollout-2026-11-14T10-00-00-.jsonl"), None);
    assert_eq!(rollout_session("rollout-x.jsonl"), None);
    assert_eq!(
        rollout_session("rollout-2026-11-14T10-00-00-a.jsonl.zst"),
        None
    );
    assert_eq!(rollout_session("notes.txt"), None);
    assert_eq!(
        rollout_session("rollout-2026-11-14T10-00-00-éa.jsonl"),
        Some("éa")
    );
}
