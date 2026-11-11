//! `cost.summary` over temp folders: transcripts are fixtures, the cache the daemon's own file,
//! and no real `~/.claude`, `~/.codex` or Swift-app file is read.

use crate::cost_ops::{self, CostSetup};
use crate::layout::Layout;
use crate::limits_ops::*;
use crate::limits_ops_tests::{rig, TempDir, NOW};
use notchling_local_api::ErrorCode;
use serde_json::json;
use std::time::{Duration, Instant};

fn wait_scanned(ops: &LimitsOps) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while ops.cost().snapshot().scanned.is_none() {
        assert!(Instant::now() < deadline, "the scan did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn opus_line(id: &str, dollars: f64) -> String {
    let tokens = (dollars / 20.0 * 1_000_000.0) as u64;
    format!(
        r#"{{"type":"assistant","timestamp":"{}","message":{{"id":"{id}","model":"claude-opus-5-5","usage":{{"output_tokens":{tokens}}}}}}}"#,
        // Today, whenever the test runs: the daemon's clock is fixed at NOW below.
        "2027-01-15T08:00:00Z"
    ) + "\n"
}

#[test]
fn cost_summary_reads_transcripts_whatever_the_switch_says() {
    let (tmp, flag, _room, ops) = rig();
    assert!(!flag.is_on());
    tmp.write(
        "claude/projects/-w-app/a.jsonl",
        &(opus_line("m1", 20.0) + &opus_line("m2", 5.0)),
    );
    let first = ops.call(COST_SUMMARY, &json!({"range": "all"})).unwrap();
    assert_eq!(first["range"], json!("all"));
    wait_scanned(&ops);
    let reply = ops.call(COST_SUMMARY, &json!({"range": "all"})).unwrap();
    assert_eq!(reply["scanning"], json!(false));
    assert!(
        (reply["summary"]["month"].as_f64().unwrap() - 25.0).abs() < 1e-6,
        "{reply}"
    );
    assert_eq!(reply["summary"]["models"][0]["name"], json!("Opus 5.5"));
    assert!(reply["by_account"].is_object());
}

#[test]
fn cost_summary_ranges() {
    let (_tmp, _flag, _room, ops) = rig();
    for ok in [
        json!({}),
        json!({"range": "7"}),
        json!({"range": 30}),
        json!({"range": "all"}),
        json!({"range": null}),
    ] {
        assert!(ops.call(COST_SUMMARY, &ok).is_ok(), "{ok}");
    }
    for bad in [
        json!({"range": "90"}),
        json!({"range": true}),
        json!({"range": "week"}),
    ] {
        let e = ops.call(COST_SUMMARY, &bad).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{bad}");
    }
    assert_eq!(
        ops.call(COST_SUMMARY, &json!({})).unwrap()["range"],
        json!("30"),
        "30 days by default"
    );
}

#[test]
fn a_cost_reply_is_cut_to_fit_a_line() {
    use notchling_engine::cost::summary::{NameCost, Summary};
    use notchling_engine::cost::tracker::Ranged;
    use notchling_engine::cost::{Snapshot, Window};
    let rows: Vec<NameCost> = (0..200)
        .map(|i| NameCost {
            name: format!("project-{i}"),
            cost: f64::from(i),
        })
        .collect();
    let summary = Summary {
        projects: rows,
        ..Summary::default()
    };
    let mut ranged = Ranged::default();
    ranged.overall.insert(Window::All, summary.clone());
    ranged
        .by_account
        .entry(Window::All)
        .or_default()
        .insert("acct".into(), summary);
    let snapshot = Snapshot {
        scanned: Some(NOW),
        scanning: false,
        ranged,
    };
    let reply = cost_ops::reply(&snapshot, Window::All);
    assert_eq!(
        reply["summary"]["projects"].as_array().unwrap().len(),
        cost_ops::MAX_ROWS
    );
    assert_eq!(
        reply["by_account"]["acct"]["projects"]
            .as_array()
            .unwrap()
            .len(),
        cost_ops::MAX_ROWS
    );
}

#[test]
fn the_cost_cache_is_the_daemons_own_document_and_never_the_swift_apps() {
    let tmp = TempDir::new("limits-cost-paths");
    let layout = Layout::resolve(None, Some(tmp.root().as_os_str()), || None).unwrap();
    let setup = CostSetup::from_layout(&layout);
    let data = setup.cache_data.expect("a fixture root keeps a cache");
    assert_eq!(data, layout.data_dir);
    assert_eq!(
        notchling_engine::cost::cache::CACHE_DOC,
        "daemon/cost-cache",
        "a row of the daemon's own, not the Swift app's cost-cache.json"
    );
    assert_eq!(setup.account_log, layout.data_dir.join("account-log.json"));
    let channels: Vec<_> = setup.roots.iter().map(|r| r.channel).collect();
    assert_eq!(
        channels,
        [
            "Claude Code",
            "Desktop",
            "Desktop",
            "Codex",
            "Antigravity CLI"
        ]
    );
    // Every root is under the fixture root.
    for r in &setup.roots {
        assert!(r.dir.starts_with(tmp.root()), "{}", r.dir.display());
    }
    let given = Layout::resolve(
        Some(std::path::Path::new("/tmp/x.sock")),
        Some(tmp.root().as_os_str()),
        || None,
    )
    .unwrap();
    assert!(
        CostSetup::from_layout(&given).cache_data.is_none(),
        "a test socket keeps none"
    );
}

fn line_for(id: &str, model: &str, output: u64) -> String {
    format!(
        r#"{{"type":"assistant","timestamp":"2027-01-15T08:00:00Z","message":{{"id":"{id}","model":"{model}","usage":{{"output_tokens":{output}}}}}}}"#
    ) + "\n"
}

#[test]
fn the_reply_lists_unpriced_and_borrowed_models() {
    let (tmp, _flag, _room, ops) = rig();
    tmp.write(
        "claude/projects/-w-app/a.jsonl",
        &(line_for("m1", "claude-new-9", 1000)
            + &line_for("m2", "claude-fable-5-3", 1_000_000)
            + &opus_line("m3", 5.0)),
    );
    ops.call(COST_SUMMARY, &json!({"range": "all"})).unwrap();
    wait_scanned(&ops);
    let reply = ops.call(COST_SUMMARY, &json!({"range": "all"})).unwrap();
    assert_eq!(reply["summary"]["unpriced"], json!(["claude-new-9"]));
    assert_eq!(reply["summary"]["borrowed"], json!(["claude-fable-5-3"]));
    // 5 for Opus 5.5 and 40 for a million Fable 5.2 output tokens.
    let month = reply["summary"]["month"].as_f64().unwrap();
    assert!((month - 45.0).abs() < 1e-6, "{reply}");
}

#[test]
fn the_daemons_catalog_file_beside_the_socket_reprices_what_is_counted() {
    use notchling_engine::cost::tracker::CLAUDE_CODE;
    use notchling_engine::cost::{Root, Window};
    let tmp = TempDir::new("cost-catalog-file");
    let layout = Layout::resolve(None, Some(tmp.root().as_os_str()), || None).unwrap();
    let from_layout = CostSetup::from_layout(&layout);
    assert_eq!(
        from_layout.catalog_file,
        layout.state_dir().join("catalog.json")
    );
    tmp.write(
        "claude/projects/-w-app/a.jsonl",
        &line_for("m1", "claude-opus-5-5", 1_000_000),
    );
    // Same catalog file, transcripts in the temp folder, no cache.
    let setup = CostSetup {
        roots: vec![Root {
            dir: tmp.path("claude/projects"),
            channel: CLAUDE_CODE,
        }],
        cache_data: None,
        ..from_layout
    };
    let month = || {
        let service = setup.service();
        service.refresh(NOW);
        service.snapshot().ranged.overall[&Window::All].month
    };
    assert!((month() - 20.0).abs() < 1e-6);
    std::fs::create_dir_all(setup.catalog_file.parent().unwrap()).unwrap();
    std::fs::write(
        &setup.catalog_file,
        r#"{"claude": {"prices": [{"match": "opus-5-5", "input": 4.0, "output": 30.0, "cache_read": 0.2}]}}"#,
    )
    .unwrap();
    assert!((month() - 30.0).abs() < 1e-6, "the override is read");
}
