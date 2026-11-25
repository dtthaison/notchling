use super::live_cache::*;
use super::{LiveModel, Tool};
use crate::sessions::testutil::TempDir;

fn model(id: &str) -> LiveModel {
    LiveModel {
        id: id.into(),
        name: Some(format!("Name of {id}")),
        ..Default::default()
    }
}

#[test]
fn the_cache_round_trips_and_is_private() {
    let tmp = TempDir::new("live-cache");
    let path = tmp.path("models-cache.json");
    let cache = LiveCache::new(path.clone());
    assert_eq!(cache.get(Tool::Claude), None);
    cache.put(
        Tool::Claude,
        1000.5,
        &[model("claude-a"), model("claude-b")],
    );
    cache.put(Tool::Antigravity, 2000.0, &[model("gemini-x")]);
    let (at, models) = cache.get(Tool::Claude).unwrap();
    assert_eq!(at, 1000.5);
    assert_eq!(models, vec![model("claude-a"), model("claude-b")]);
    let (at, models) = cache.get(Tool::Antigravity).unwrap();
    assert_eq!((at, models), (2000.0, vec![model("gemini-x")]));
    assert_eq!(cache.get(Tool::Codex), None);
    cache.put(Tool::Claude, 3000.0, &[model("claude-c")]);
    assert_eq!(cache.get(Tool::Claude).unwrap().1, vec![model("claude-c")]);
    assert_eq!(cache.get(Tool::Antigravity).unwrap().0, 2000.0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let left: Vec<_> = std::fs::read_dir(tmp.root())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left.len(), 1, "no temp file is left behind: {left:?}");
}

#[test]
fn a_missing_or_broken_cache_reads_as_none() {
    let tmp = TempDir::new("live-cache-broken");
    let missing = LiveCache::new(tmp.path("nope/cache.json"));
    assert_eq!(missing.get(Tool::Claude), None);
    for (name, body) in [
        ("empty.json", ""),
        ("garbage.json", "<html>"),
        ("array.json", "[]"),
        (
            "shape.json",
            r#"{"claude": {"at": "yesterday", "models": 3}}"#,
        ),
    ] {
        let path = tmp.write(name, body);
        assert_eq!(LiveCache::new(path).get(Tool::Claude), None, "{name}");
    }
    let path = tmp.write("fix.json", "<html>");
    let cache = LiveCache::new(path);
    cache.put(Tool::Claude, 5.0, &[model("claude-a")]);
    assert_eq!(cache.get(Tool::Claude).unwrap().0, 5.0);
    let deep = LiveCache::new(tmp.path("a/b/cache.json"));
    deep.put(Tool::Claude, 1.0, &[]);
    assert_eq!(deep.get(Tool::Claude), Some((1.0, vec![])));
}

#[test]
fn refresh_is_due_after_a_day() {
    assert!(refresh_due(None, 100.0));
    assert!(!refresh_due(Some(100.0), 100.0));
    assert!(!refresh_due(Some(100.0), 100.0 + 86_400.0));
    assert!(refresh_due(Some(100.0), 100.0 + 86_400.5));
    assert!(refresh_due(Some(100_000.0), 10.0));
}

#[test]
fn puts_from_two_threads_lose_nothing() {
    let tmp = TempDir::new("live-cache-threads");
    let path = tmp.path("cache.json");
    let workers: Vec<_> = [Tool::Claude, Tool::Antigravity]
        .into_iter()
        .map(|tool| {
            let path = path.clone();
            std::thread::spawn(move || {
                let cache = LiveCache::new(path);
                for i in 0..200 {
                    cache.put(tool, f64::from(i), &[model(&format!("{tool:?}-{i}"))]);
                }
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }
    let cache = LiveCache::new(path);
    assert_eq!(cache.get(Tool::Claude).unwrap().0, 199.0);
    assert_eq!(cache.get(Tool::Antigravity).unwrap().0, 199.0);
    let left = std::fs::read_dir(tmp.root()).unwrap().count();
    assert_eq!(left, 1, "no temp file is left behind");
}

#[test]
fn one_entry_that_does_not_decode_is_skipped_and_a_short_one_still_reads() {
    let tmp = TempDir::new("live-cache-entries");
    let path = tmp.write(
        "cache.json",
        r#"{"claude": {"at": 7.0, "models": [{"id": "claude-a"}, {"id": 5}, "junk", {"id": "claude-b", "name": "B", "efforts": ["high"]}]}}"#,
    );
    let (at, models) = LiveCache::new(path).get(Tool::Claude).unwrap();
    assert_eq!(at, 7.0);
    let ids: Vec<_> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["claude-a", "claude-b"]);
    assert!(models[0].efforts.is_empty());
    assert_eq!(models[1].efforts, ["high"]);
}
