use crate::cost_ops::{self, CostSetup};
use crate::layout::Layout;
use crate::limits_ops::*;
use crate::limits_ops_tests::{rig, TempDir, NOW};
use notchling_engine::catalog::Catalog;
use notchling_local_api::ErrorCode;
use serde_json::json;
use std::sync::Arc;
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
    let setup = CostSetup::from_layout(&layout, Arc::new(Catalog::built_in()));
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
        CostSetup::from_layout(&given, Arc::new(Catalog::built_in()))
            .cache_data
            .is_none(),
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
    let month = reply["summary"]["month"].as_f64().unwrap();
    assert!((month - 45.0).abs() < 1e-6, "{reply}");
}

#[test]
fn cost_prices_from_the_daemons_one_catalog_with_the_file_beside_the_socket_laid_over_it() {
    use crate::acting_ops_tests::FakeRoom;
    use crate::catalog_ops::CatalogService;
    use crate::switches::{SwitchFlag, ACCOUNTS, ACCOUNTS_OFF};
    use notchling_engine::cost::tracker::CLAUDE_CODE;
    use notchling_engine::cost::{Root, Window};
    let tmp = TempDir::new("cost-catalog-file");
    let layout = Layout::resolve(None, Some(tmp.root().as_os_str()), || None).unwrap();
    tmp.write(
        "notchling/catalog.json",
        r#"{"claude": {"prices": [{"match": "opus-5-5", "input": 4.0, "output": 30.0, "cache_read": 0.2}]}}"#,
    );
    tmp.write(
        "claude/projects/-w-app/a.jsonl",
        &line_for("m1", "claude-opus-5-5", 1_000_000),
    );
    let room = Arc::new(FakeRoom::default());
    let flag = SwitchFlag::off(ACCOUNTS, ACCOUNTS_OFF);
    let accounts =
        crate::accounts_ops::AccountsOps::for_daemon(&layout, flag.clone(), room.clone());
    let catalog = CatalogService::for_daemon(&layout, (accounts.store(), flag), room);
    let from_layout = CostSetup::from_layout(&layout, catalog.shared_catalog());
    let setup = CostSetup {
        roots: vec![Root {
            dir: tmp.path("claude/projects"),
            channel: CLAUDE_CODE,
        }],
        cache_data: None,
        ..from_layout
    };
    assert!(
        Arc::ptr_eq(&setup.catalog, &catalog.shared_catalog()),
        "cost shares the service's catalog instead of loading its own"
    );
    let service = setup.service();
    service.refresh(NOW);
    let month = service.snapshot().ranged.overall[&Window::All].month;
    assert!((month - 30.0).abs() < 1e-6, "the override is read: {month}");
}

#[test]
fn each_unpriced_model_is_logged_once_per_run() {
    let lines = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let (tmp, _flag, _room, ops) = rig();
    let sink = lines.clone();
    let ops = ops.with_unpriced_sink(Box::new(move |l| sink.lock().unwrap().push(l.to_string())));
    tmp.write(
        "claude/projects/-w-app/a.jsonl",
        &(line_for("m1", "claude-new-9", 1000) + &line_for("m2", "claude-newer-9", 1000)),
    );
    ops.call(COST_SUMMARY, &json!({"range": "all"})).unwrap();
    wait_scanned(&ops);
    for _ in 0..3 {
        ops.call(COST_SUMMARY, &json!({"range": "all"})).unwrap();
        ops.call(COST_SUMMARY, &json!({"range": "7"})).unwrap();
    }
    let lines = lines.lock().unwrap();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].contains("claude-new-9") && lines[1].contains("claude-newer-9"));
    assert!(lines.iter().all(|l| l.contains("no price")), "{lines:?}");
}
