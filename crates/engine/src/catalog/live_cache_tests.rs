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
    // A second put for one tool leaves the other's entry.
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
    // A put over a broken file starts it again.
    let path = tmp.write("fix.json", "<html>");
    let cache = LiveCache::new(path);
    cache.put(Tool::Claude, 5.0, &[model("claude-a")]);
    assert_eq!(cache.get(Tool::Claude).unwrap().0, 5.0);
    // A put into a folder that doesn't exist yet makes it.
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
    // A clock that went backwards doesn't leave a list fresh forever.
    assert!(refresh_due(Some(100_000.0), 10.0));
}
