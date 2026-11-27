use serde_json::{json, Value};

use super::*;

const NOW: i64 = 1_794_096_000_000;

fn env() -> Env {
    Env {
        now_ms: NOW,
        vault_on: true,
        shown_ms: 5_000,
        node: None,
        ..Env::default()
    }
}

fn job(kind: &str, answer_on: &str, change: Value, facts: Value) -> Value {
    json!({
        "id": "c9", "kind": kind, "target": "job-1", "state": "waiting",
        "change": change, "facts": facts,
        "tool": "claude", "verified": true, "session": "0b1c2d3e4f5a",
        "expiresMs": NOW + 14 * 60_000, "appliedBy": "daemon", "answerOn": answer_on
    })
}

fn dispatch_facts() -> Value {
    json!([
        {"line": "consent-job-dispatch", "args": {"tool": "shell", "machine": "build", "folder": "/work/a"}},
        {"line": "consent-job-machine-online", "args": {"machine": "build", "organization": "Personal"}},
        {"line": "consent-job-model", "args": {"model": "opus", "effort": "high"}},
        {"line": "consent-job-secrets", "args": {"names": "deploy-token, db-pass"}},
    ])
}

#[test]
fn a_dispatch_says_the_machine_the_model_and_the_secrets() {
    let view = job(
        "dispatch",
        "mac",
        json!({"action": "dispatch", "tool": "shell", "machine": "build", "message": "make test"}),
        dispatch_facts(),
    );
    let card = ConsentCard::build(&view, &env()).expect("card");
    assert_eq!(
        card.facts,
        vec![
            t::consent::card_fact_job_dispatch(
                t::consent::card_job_tool_shell(),
                "build",
                "/work/a"
            ),
            t::consent::card_fact_job_machine_online("build", "Personal"),
            t::consent::card_fact_job_model("opus"),
            t::consent::card_fact_job_secrets("deploy-token, db-pass"),
        ]
    );
    assert!(card.facts[3].contains("deploy-token, db-pass"));
}

#[test]
fn an_offline_machine_says_the_job_waits() {
    let view = job(
        "dispatch",
        "mac",
        json!({"action": "dispatch", "tool": "claude", "machine": "build"}),
        json!([
            {"line": "consent-job-dispatch", "args": {"tool": "claude", "machine": "build", "folder": "/work/a"}},
            {"line": "consent-job-machine-offline", "args": {"machine": "build", "organization": "Personal"}},
        ]),
    );
    let card = ConsentCard::build(&view, &env()).expect("card");
    assert_eq!(
        card.facts,
        vec![
            t::consent::card_fact_job_dispatch(t::consent::card_tool_claude(), "build", "/work/a"),
            t::consent::card_fact_job_machine_offline("build", "Personal"),
        ]
    );
    assert!(card.facts[1].contains("waits"));
}

#[test]
fn a_tail_a_return_and_a_stop_name_the_job_its_machine_and_folder() {
    let args = json!({"job": "j-7", "machine": "build", "folder": "/work/a"});
    let cases = [
        (
            "job",
            "consent-job-tail",
            t::consent::card_fact_job_tail("j-7", "build", "/work/a"),
        ),
        (
            "job",
            "consent-job-return",
            t::consent::card_fact_job_return("j-7", "build", "/work/a"),
        ),
        (
            "job_stop",
            "consent-job-stop",
            t::consent::card_fact_job_stop("j-7", "build", "/work/a"),
        ),
    ];
    for (kind, line, want) in cases {
        let view = job(
            kind,
            "mac",
            json!({"action": "x", "machine": "build"}),
            json!([{"line": line, "args": args}]),
        );
        let card = ConsentCard::build(&view, &env()).expect("card");
        assert_eq!(card.facts, vec![want.clone()], "{line}");
        assert!(want.contains("j-7") && want.contains("build"), "{line}");
    }
}

#[test]
fn a_job_line_missing_its_words_is_left_out_not_shown_half_made() {
    let view = job(
        "job_stop",
        "mac_or_phone",
        json!({"action": "stop", "machine": "build"}),
        json!([{"line": "consent-job-stop", "args": {"job": "j-7"}}]),
    );
    let card = ConsentCard::build(&view, &env()).expect("card");
    assert!(card.facts.is_empty());
}
