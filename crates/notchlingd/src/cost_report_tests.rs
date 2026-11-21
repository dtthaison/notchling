use crate::cost_ops::CostSetup;
use crate::cost_report_cli::{code_of, parse, request, shown, Args, DONE, REFUSED, TROUBLE};
use crate::cost_report_ops::{CostReportOps, COST_REPORT};
use crate::cost_report_text::render;
use crate::op_group::OpGroup;
use notchling_engine::catalog::Catalog;
use notchling_engine::cost::tracker::{Root, CLAUDE_CODE};
use notchling_local_api::{ApiError, Caller, ClientError, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const NOW: f64 = 1_795_176_000.0;

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Tmp {
        let dir = std::env::temp_dir().join(format!("nl-cost-report-{tag}-{}", std::process::id()));
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

fn stamp(ts: f64) -> String {
    let secs = ts as i64;
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    let (mut y, mut d) = (1970i64, days);
    loop {
        let len = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
            366
        } else {
            365
        };
        if d < len {
            break;
        }
        d -= len;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0;
    while d >= months[m] {
        d -= months[m];
        m += 1;
    }
    format!(
        "{y:04}-{:02}-{:02}T{:02}:{:02}:{:02}.000Z",
        m + 1,
        d + 1,
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

fn line(id: &str, ts: f64, model: &str, read: u64, w5: u64, w1: u64) -> String {
    format!(
        r#"{{"type":"assistant","timestamp":"{}","requestId":"r-{id}","message":{{"id":"{id}","model":"{model}","usage":{{"input_tokens":10,"output_tokens":400,"cache_read_input_tokens":{read},"cache_creation_input_tokens":{},"cache_creation":{{"ephemeral_5m_input_tokens":{w5},"ephemeral_1h_input_tokens":{w1}}}}}}}}}"#,
        stamp(ts),
        w5 + w1
    ) + "\n"
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, text).expect("write");
}

fn fixture(tmp: &Tmp) {
    let proj = tmp.path().join("projects/-tmp-demo");
    let mut main = String::new();
    for i in 0..4 {
        main += &line(
            &format!("m{i}"),
            NOW - 90_000.0 + i as f64 * 60.0,
            "claude-sonnet-5-5",
            80_000,
            0,
            10_000,
        );
    }
    write(&proj.join("s1.jsonl"), &main);
    for (n, agent) in ["agent-b1", "agent-b2"].into_iter().enumerate() {
        let mut text = String::new();
        for i in 0..8 {
            let gap = if i == 4 { 900.0 } else { 20.0 };
            text += &line(
                &format!("{agent}-{i}"),
                NOW - 80_000.0
                    + n as f64 * 1_000.0
                    + i as f64 * 20.0
                    + if i >= 4 { gap } else { 0.0 },
                "claude-opus-5-5",
                60_000,
                if i == 4 { 120_000 } else { 20_000 },
                0,
            );
        }
        write(&proj.join(format!("s1/subagents/{agent}.jsonl")), &text);
        write(
            &proj.join(format!("s1/subagents/{agent}.meta.json")),
            r#"{"agentType":"builder","model":"opus","effort":"high"}"#,
        );
    }
}

fn ops_for(tmp: &Tmp) -> CostReportOps {
    let setup = CostSetup {
        roots: vec![Root {
            dir: tmp.path().join("projects"),
            channel: CLAUDE_CODE,
        }],
        cache_data: None,
        account_log: tmp.path().join("account-log.json"),
        catalog: Arc::new(Catalog::built_in()),
    };
    CostReportOps::new(setup.service()).with_clock(Arc::new(|| NOW))
}

fn finding<'a>(reply: &'a Value, id: &str) -> Option<&'a Value> {
    reply["report"]["findings"]
        .as_array()?
        .iter()
        .chain(reply["report"]["not_worth"].as_array()?)
        .find(|f| f["id"] == id)
}

#[test]
fn the_report_op_reads_transcripts_and_ranks_what_to_change() {
    let tmp = Tmp::new("e2e");
    fixture(&tmp);
    let ops = ops_for(&tmp);
    assert_eq!(ops.spec(COST_REPORT), Some(OpSpec::AGENT_READ));
    let reply = ops.report(&json!({})).expect("report");
    assert_eq!(reply["complete"], true);
    assert_eq!(reply["report"]["days"], 14);
    assert_eq!(reply["report"]["totals"]["calls"], 20);
    assert_eq!(reply["report"]["cache"]["main"]["one_hour"], 1);
    assert_eq!(reply["report"]["cache"]["sub"]["five_min"], 2);
    let premium = finding(&reply, "premium_subagents").expect("premium");
    assert_eq!(premium["detail"]["target"], "claude-sonnet-5-5");
    assert!(premium["saving"].as_f64().expect("saving") > 0.0);
    let cold = finding(&reply, "cold_resumes").expect("cold");
    let sub = cold["evidence"]
        .as_array()
        .expect("evidence")
        .iter()
        .find(|e| e["name"] == "sub_rebuilds")
        .expect("sub_rebuilds");
    assert_eq!(sub["value"], 2.0);
    let total = reply["report"]["totals"]["cost"].as_f64().expect("cost");
    let saving = reply["report"]["saving"].as_f64().unwrap_or(0.0);
    assert!(saving > 0.0 && saving < total);
}

#[test]
fn the_report_op_reads_through_the_group_and_checks_its_arguments() {
    let tmp = Tmp::new("args");
    fixture(&tmp);
    let ops = ops_for(&tmp);
    let caller = Caller {
        tier: Tier::Agent,
        client: ClientKind::Cli,
        peer: None,
        pid: None,
    };
    let via_group = ops
        .call(
            &caller,
            COST_REPORT,
            &json!({"days": 3, "plan": "subscription"}),
        )
        .expect("call");
    assert_eq!(via_group["report"]["days"], 3);
    assert_eq!(via_group["report"]["plan"], "subscription");
    assert_eq!(via_group["report"]["money"], "api_equivalent");
    for bad in [
        json!({"days": 0}),
        json!({"days": 31}),
        json!({"days": "x"}),
        json!({"plan": "free"}),
    ] {
        let err = ops.report(&bad).expect_err("refused");
        assert_eq!(err.code, ErrorCode::BadRequest, "{bad}");
    }
    let other: Result<Value, ApiError> = ops.call(&caller, "cost.nope", &json!({}));
    assert!(other.is_err());
}

#[test]
fn the_text_names_the_findings_and_labels_the_money() {
    let tmp = Tmp::new("text");
    fixture(&tmp);
    let reply = ops_for(&tmp).report(&json!({})).expect("report");
    let text = render(&reply);
    assert!(text.starts_with("Cost Check: the last 14 days"), "{text}");
    assert!(text.contains("API-equivalent"), "{text}");
    assert!(text.contains("Opus or Fable on helper agents"), "{text}");
    assert!(text.contains("Cache rebuilt after an idle gap"), "{text}");
    assert!(text.contains("Where it goes"), "{text}");
    assert!(
        text.contains("Many builders at once: never more than 2"),
        "{text}"
    );
    assert!(
        text.contains("Long helper runs and big contexts: no run went past 100 calls"),
        "{text}"
    );
    assert!(!text.contains("saving unknown"), "{text}");
    assert!(text.contains("claude-opus-5-5"), "{text}");
    assert_eq!(shown(&reply, false), text);
    let parsed: Value = serde_json::from_str(&shown(&reply, true)).expect("json");
    assert_eq!(
        parsed["report"]["findings"][0]["id"],
        reply["report"]["findings"][0]["id"]
    );
    assert_eq!(
        parsed["report"]["totals"]["calls"],
        reply["report"]["totals"]["calls"]
    );
    assert_eq!(parsed["ledger_calls"], reply["ledger_calls"]);
}

#[test]
fn a_report_with_nothing_priced_says_unknown_not_zero() {
    let tmp = Tmp::new("unknown");
    let proj = tmp.path().join("projects/-tmp-demo");
    write(
        &proj.join("s1.jsonl"),
        &line("m1", NOW - 1_000.0, "claude-mystery-9", 5_000, 0, 0),
    );
    let reply = ops_for(&tmp).report(&json!({})).expect("report");
    assert!(reply["report"]["totals"]["cost"].is_null());
    let text = render(&reply);
    assert!(text.contains("Spend: unknown"), "{text}");
    assert!(text.contains("no price"), "{text}");
    assert!(text.contains("Nothing stands out"), "{text}");
}

#[test]
fn an_empty_window_still_reports() {
    let tmp = Tmp::new("empty");
    let reply = ops_for(&tmp).report(&json!({})).expect("report");
    assert_eq!(reply["report"]["totals"]["calls"], 0);
    assert!(render(&reply).contains("Nothing stands out"));
}

#[test]
fn the_command_line_parses_report_and_its_flags() {
    let p = |a: &[&str]| parse(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(
        p(&["report"]),
        Ok(Args {
            days: None,
            plan: None,
            json: false,
            socket: None
        })
    );
    let full = p(&[
        "report",
        "--days",
        "7",
        "--plan",
        "subscription",
        "--json",
        "--socket",
        "/x",
    ])
    .expect("parse");
    assert_eq!(full.days, Some(7));
    assert_eq!(full.plan.as_deref(), Some("subscription"));
    assert!(full.json);
    assert_eq!(full.socket, Some(PathBuf::from("/x")));
    assert_eq!(request(&full), json!({"days": 7, "plan": "subscription"}));
    assert_eq!(request(&p(&["report"]).expect("parse")), json!({}));
    for bad in [
        &["setup"][..],
        &[],
        &["report", "--days"],
        &["report", "--days", "0"],
        &["report", "--days", "99"],
        &["report", "--plan", "free"],
        &["report", "--wat"],
    ] {
        assert!(p(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn errors_map_to_exit_codes() {
    let api = |code| ClientError::Api(ApiError::new(code, "no"));
    assert_eq!(code_of(&api(ErrorCode::BadRequest)), REFUSED);
    assert_eq!(code_of(&api(ErrorCode::Failed)), REFUSED);
    assert_eq!(code_of(&api(ErrorCode::UnknownOp)), TROUBLE);
    assert_eq!(code_of(&ClientError::Timeout), TROUBLE);
    assert_eq!(DONE, 0);
}
