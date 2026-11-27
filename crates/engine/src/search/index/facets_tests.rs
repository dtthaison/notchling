use super::*;
use crate::cost::account_log;
use crate::sessions::testutil::TempDir;
use std::fs::OpenOptions;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn line(text: &str, cwd: &str, at: &str) -> String {
    format!(
        r#"{{"type":"user","cwd":"{cwd}","timestamp":"{at}","message":{{"role":"user","content":"{text}"}}}}"#
    )
}

fn write(dir: &TempDir, rel: &str, lines: &[String]) {
    let path = dir.write(rel, &(lines.join("\n") + "\n"));
    let file = OpenOptions::new().write(true).open(&path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs_f64(now() - 600.0))
        .unwrap();
}

fn rig() -> (TempDir, IndexRoots, AccountOf) {
    let dir = TempDir::new("idx-facets");
    write(
        &dir,
        "claude/projects/-w-app/a1.jsonl",
        &[line(
            "zebra migration",
            "/w/app",
            "2026-11-10T10:00:00.000Z",
        )],
    );
    write(
        &dir,
        "lane/projects/-w-site/b1.jsonl",
        &[line("zebra layout", "/w/site", "2026-11-10T11:00:00.000Z")],
    );
    dir.write(
        "account-log.json",
        r#"[{"at":"2026-11-01T00:00:00Z","id":"work"},{"at":"2026-11-12T00:00:00Z","id":"home"}]"#,
    );
    let entries = account_log::load(&dir.path("account-log.json"));
    let accounts: AccountOf = Arc::new(move |config_dir, at| match config_dir {
        Some(lane) => format!("lane:{lane}"),
        None => account_log::account_at(at, &entries),
    });
    let roots = IndexRoots {
        claude: vec![
            ClaudeRoot {
                projects: dir.path("claude/projects"),
                config_dir: None,
            },
            ClaudeRoot {
                projects: dir.path("lane/projects"),
                config_dir: Some("/lane/one".into()),
            },
        ],
        codex_sessions: None,
        titled: Vec::new(),
    };
    (dir, roots, accounts)
}

fn only(account: &str) -> Filter {
    Filter {
        account: Some(vec![account.into()]),
        ..Filter::default()
    }
}

#[test]
fn account_is_the_one_active_at_start() {
    let (_dir, roots, accounts) = rig();
    let index = SessionIndex::with_range(None, None).with_accounts(accounts);
    index.refresh(&roots, now());
    let work = index.search_filtered(&["zebra"], &only("work"), now(), 10);
    assert_eq!(work.len(), 1);
    assert_eq!(work[0].meta.folder, "/w/app");
    assert_eq!(work[0].meta.account.as_deref(), Some("work"));
    assert!(index
        .search_filtered(&["zebra"], &only("home"), now(), 10)
        .is_empty());
    let lane = index.search_filtered(&["zebra"], &only("lane:/lane/one"), now(), 10);
    assert_eq!(lane.len(), 1);
    assert_eq!(lane[0].meta.folder, "/w/site");
}

#[test]
fn without_a_resolver_the_account_is_unknown() {
    let (_dir, roots, _) = rig();
    let index = SessionIndex::with_range(None, None);
    index.refresh(&roots, now());
    assert_eq!(
        index
            .search_filtered(&["zebra"], &only("unknown"), now(), 10)
            .len(),
        2
    );
}

#[test]
fn an_empty_query_with_a_filter_lists_the_matching_latest() {
    let (_dir, roots, accounts) = rig();
    let index = SessionIndex::with_range(None, None).with_accounts(accounts);
    index.refresh(&roots, now());
    let none: [&str; 0] = [];
    let hits = index.search_filtered(&none, &only("work"), now(), 10);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].meta.folder, "/w/app");
    let projects = Filter {
        project: Some(vec!["/w/site".into()]),
        ..Filter::default()
    };
    assert_eq!(index.search_filtered(&none, &projects, now(), 10).len(), 1);
}

#[test]
fn a_filter_still_applies_when_the_index_is_scanned() {
    let (_dir, roots, accounts) = rig();
    let index = SessionIndex::with_range(None, None).with_accounts(accounts);
    index.refresh(&roots, now());
    let hits = index.search_filtered(&["zebr"], &only("work"), now(), 10);
    assert_eq!(hits.len(), 1);
}

#[test]
fn facet_values_count_prefix_and_keep() {
    let (_dir, roots, accounts) = rig();
    let index = SessionIndex::with_range(None, None).with_accounts(accounts);
    index.refresh(&roots, now());
    let all = index.facet_values("project", "", 20, &|_| true);
    let labels: Vec<&str> = all.iter().map(|v| v.label.as_str()).collect();
    assert_eq!(labels, ["app", "site"]);
    assert_eq!(all[0].value, "/w/app");
    assert_eq!(all[0].count, 1);
    let prefixed = index.facet_values("project", "SI", 20, &|_| true);
    assert_eq!(prefixed.len(), 1);
    assert_eq!(prefixed[0].label, "site");
    let kept = index.facet_values("project", "", 20, &|m| m.folder == "/w/app");
    assert_eq!(kept.len(), 1);
    let tools = index.facet_values("tool", "", 20, &|_| true);
    assert_eq!((tools[0].value.as_str(), tools[0].count), ("claude", 2));
    assert_eq!(index.facet_values("project", "", 1, &|_| true).len(), 1);
    assert!(index.facet_values("owner", "", 20, &|_| true).is_empty());
}
