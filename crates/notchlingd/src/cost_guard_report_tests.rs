use crate::cost_ops::CostSetup;
use crate::cost_report_ops::CostReportOps;
use crate::cost_report_text::render;
use notchling_engine::catalog::Catalog;
use notchling_engine::cost::tracker::{Root, CLAUDE_CODE};
use notchling_engine::guard::{self, policy};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const NOW: f64 = 1_795_176_000.0;
const DAY: f64 = 86_400.0;

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Tmp {
        let dir = std::env::temp_dir().join(format!("nl-cost-guard-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        Tmp(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn ops_for(tmp: &Tmp) -> CostReportOps {
    let setup = CostSetup {
        roots: vec![Root {
            dir: tmp.path().join("projects"),
            channel: CLAUDE_CODE,
        }],
        cache_data: Some(tmp.path().join("data")),
        account_log: tmp.path().join("account-log.json"),
        catalog: Arc::new(Catalog::built_in()),
    };
    CostReportOps::new(setup.service()).with_clock(Arc::new(|| NOW))
}

fn hook(tmp: &Tmp, at: f64, session: &str, tool: &str, input: Value) {
    let dir = tmp.path().join("data").join(guard::DIR);
    policy::write(&dir, 5).expect("policy");
    let event = json!({
        "hook_event_name": "PreToolUse",
        "session_id": session,
        "tool_name": tool,
        "tool_use_id": format!("toolu_{at}"),
        "cwd": "/work",
        "tool_input": input
    });
    guard::record(event.to_string().as_bytes(), &dir, at);
}

fn fixture(tmp: &Tmp) {
    let sleep = json!({"command": "sleep 60"});
    let tree = json!({"command": "ls -R"});
    let opus = json!({"description": "add a column", "model": "opus"});
    hook(tmp, NOW - 3.0 * DAY, "sess-a", "Bash", sleep.clone());
    hook(tmp, NOW - 2.5 * DAY, "sess-a", "Bash", sleep.clone());
    hook(tmp, NOW - 2.5 * DAY, "sess-b", "Bash", sleep);
    hook(tmp, NOW - 1.0 * DAY, "sess-b", "Bash", tree);
    hook(tmp, NOW - 1.0 * DAY, "sess-c", "Agent", opus);
    hook(
        tmp,
        NOW - 40.0 * DAY,
        "sess-old",
        "Bash",
        json!({"command": "sleep 60"}),
    );
}

#[test]
fn the_report_counts_what_the_guard_would_have_done_per_rule_and_session() {
    let tmp = Tmp::new("counts");
    fixture(&tmp);
    let reply = ops_for(&tmp).report(&json!({})).expect("report");
    let section = &reply["report"]["guard"];
    assert_eq!(section["days"], 14);
    assert_eq!(section["total"], 5);
    assert_eq!(section["sessions"], 3);
    let rules: Vec<(String, u64, u64)> = section["rules"]
        .as_array()
        .expect("rules")
        .iter()
        .map(|r| {
            (
                r["rule"].as_str().expect("rule").to_string(),
                r["count"].as_u64().expect("count"),
                r["sessions"].as_u64().expect("sessions"),
            )
        })
        .collect();
    assert_eq!(
        rules,
        vec![
            ("model".to_string(), 1, 1),
            ("tree".to_string(), 1, 1),
            ("poll".to_string(), 3, 2)
        ]
    );
}

#[test]
fn the_text_has_a_section_for_it() {
    let tmp = Tmp::new("text");
    fixture(&tmp);
    let reply = ops_for(&tmp).report(&json!({})).expect("report");
    let text = render(&reply);
    let section: Vec<&str> = text
        .lines()
        .skip_while(|l| !l.starts_with("What the guard would have done"))
        .collect();
    assert_eq!(
        section,
        vec![
            "What the guard would have done (last 14 days)",
            "  5 calls would have been noted or changed, in 3 sessions. The guard only records: no call was changed, denied or delayed.",
            "  Premium model on routine work: 1 calls in 1 sessions",
            "  Directory listing without a limit: 1 calls in 1 sessions",
            "  Waiting by sleeping or looping: 3 calls in 2 sessions",
        ],
        "{text}"
    );
}

#[test]
fn a_shorter_window_counts_fewer() {
    let tmp = Tmp::new("window");
    fixture(&tmp);
    let reply = ops_for(&tmp).report(&json!({"days": 2})).expect("report");
    assert_eq!(reply["report"]["guard"]["days"], 2);
    assert_eq!(reply["report"]["guard"]["total"], 2);
    assert!(render(&reply).contains("(last 2 days)"));
}

#[test]
fn with_no_guard_events_the_text_has_no_section() {
    let tmp = Tmp::new("none");
    let reply = ops_for(&tmp).report(&json!({})).expect("report");
    assert_eq!(reply["report"]["guard"]["total"], 0);
    assert!(!render(&reply).contains("What the guard would have done"));
}
