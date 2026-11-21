use super::{args_of, check, run_remote};
use crate::contract::{self, Failure};
use notchling_local_api::{ops, ApiError, ClientError, ErrorCode};
use serde_json::{json, Value};
use std::path::Path;

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn status() -> Value {
    json!({
        "me": "aaaaaaaaaaaaaaaa",
        "members": [
            {"id": "aaaaaaaaaaaaaaaa", "name": "mac", "state": "active"},
            {"id": "bbbbbbbbbbbbbbbb", "name": "Worker", "state": "active"},
            {"id": "cccccccccccccccc", "name": "old", "state": "removed"},
        ],
    })
}

struct Run {
    code: i32,
    out: String,
    err: String,
    calls: Vec<(String, Value)>,
}

fn run(rest: &[&str], machine: &str, reply: Result<Value, ClientError>) -> Run {
    let mut calls: Vec<(String, Value)> = Vec::new();
    let mut reply = Some(reply);
    let mut call = |op: &str, args: Value| -> Result<Value, ClientError> {
        calls.push((op.to_string(), args));
        match op {
            ops::CLUSTER_STATUS => Ok(status()),
            _ => reply.take().expect("one dispatch"),
        }
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_remote(
        machine,
        &strings(rest),
        (Path::new("/work"), None),
        &mut call,
        (&mut out, &mut err),
    );
    Run {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
        calls,
    }
}

#[test]
fn check_takes_a_whole_agy_review() {
    let list = strings(&[
        "agy",
        "--folder",
        "/srv",
        "--timeout",
        "600",
        "--json",
        "--",
        "review",
    ]);
    let parsed = check(&list).unwrap();
    assert_eq!(parsed.options.folder, "/srv");
    assert!(parsed.json);
}

#[test]
fn check_refuses_what_must_not_run_on_another_machine() {
    let cases: [&[&str]; 9] = [
        &["agy", "--folder", "/srv", "--write", "--", "x"],
        &["agy", "--folder", "/srv", "--out", "/tmp/x", "--", "x"],
        &["agy", "--folder", "/srv", "--prompt-file", "/etc/hosts"],
        &["codex", "--folder", "/srv", "--", "x"],
        &["agy", "--folder", "/srv", "--timeout", "2101", "--", "x"],
        &["agy", "--", "x"],
        &["agy", "--folder", "/srv"],
        &["emacs", "--folder", "/srv", "--", "x"],
        &[],
    ];
    for case in cases {
        let got = check(&strings(case));
        assert_eq!(got.unwrap_err().code, ErrorCode::BadRequest, "{case:?}");
    }
    let edge = strings(&["agy", "--folder", "/srv", "--timeout", "2100", "--", "x"]);
    assert!(check(&edge).is_ok());
    let after = strings(&["agy", "--folder", "/srv", "--", "--write", "--out"]);
    assert!(check(&after).is_ok(), "words after -- are the prompt");
}

#[test]
fn args_are_a_list_of_strings() {
    assert_eq!(args_of(&json!(["a", "b"])).unwrap(), strings(&["a", "b"]));
    for bad in [json!("a"), json!(["a", 1]), json!(null), json!({})] {
        let got: Result<Vec<String>, ApiError> = args_of(&bad);
        assert!(got.is_err(), "{bad}");
    }
}

#[test]
fn an_unknown_node_is_exit_two_and_dispatches_nothing() {
    let got = run(&["agy", "--", "hi"], "nobody", Ok(json!({})));
    assert_eq!(got.code, 2);
    assert_eq!(got.err, "No node named nobody. (bad_request)\n");
    assert_eq!(got.calls.len(), 1);
    let removed = run(&["agy", "--", "hi"], "old", Ok(json!({})));
    assert_eq!(removed.code, 2, "a removed node can't be picked");
}

#[test]
fn a_bad_command_is_exit_two_before_any_call() {
    let got = run(&["agy", "--write", "--", "hi"], "worker", Ok(json!({})));
    assert_eq!(got.code, 2);
    assert!(got.err.contains("--write"), "{}", got.err);
    assert!(got.calls.is_empty());
    let none = run(&["agy"], "worker", Ok(json!({})));
    assert_eq!(none.code, 2);
    assert!(none.calls.is_empty());
}

#[test]
fn the_warning_comes_first_then_the_jobs_output_and_its_exit_code() {
    let reply = json!({
        "job": "j-1", "state": "done", "warning": "Worker runs agy 1.5.0, the Mac 1.5.1",
        "exit": 0, "out": "the review\n", "err": "note\n",
    });
    let got = run(
        &["agy", "--folder", "/srv", "--", "review"],
        "WORKER",
        Ok(reply),
    );
    assert_eq!(got.code, 0);
    assert_eq!(got.out, "the review\n");
    assert_eq!(
        got.err, "Worker runs agy 1.5.0, the Mac 1.5.1\nnote\n",
        "the warning is printed before the job's own stderr"
    );
    let (op, args) = &got.calls[1];
    assert_eq!(op, ops::CLUSTER_DISPATCH);
    assert_eq!(args["node"], "bbbbbbbbbbbbbbbb");
    assert!(args["key"].as_str().is_some_and(|k| !k.is_empty()));
    assert_eq!(
        args["delegate"],
        json!([
            "agy",
            "--folder",
            "/srv",
            "--timeout",
            "1800",
            "--",
            "review"
        ])
    );
}

#[test]
fn the_jobs_exit_code_is_the_commands() {
    let reply = json!({"job": "j-2", "state": "failed", "exit": 1, "out": "", "err": "boom\n"});
    let got = run(&["agy", "--", "x"], "bbbbbbbbbbbbbbbb", Ok(reply));
    assert_eq!(got.code, 1);
    assert_eq!(got.err, "boom\nThe job failed. (failed)\n");
}

#[test]
fn a_refusal_from_the_cluster_is_printed_and_exit_one() {
    let refused = ClientError::Api(ApiError::new(ErrorCode::Failed, "Worker is offline"));
    let got = run(&["agy", "--", "x"], "worker", Err(refused));
    assert_eq!(got.code, 1);
    assert_eq!(got.err, "Worker is offline (failed)\n");
    assert_eq!(got.out, "");
    let silent = run(&["agy", "--", "x"], "worker", Err(ClientError::Timeout));
    assert_eq!(silent.code, 3);
}

#[test]
fn a_reply_without_an_exit_code_says_where_the_job_stands() {
    let reply = json!({"job": "j-3", "state": "running"});
    let got = run(&["agy", "--", "x"], "worker", Ok(reply));
    assert_eq!(got.code, 1);
    assert_eq!(got.err, "The job j-3 is running. (failed)\n");
}

fn only_json(text: &str) -> Value {
    serde_json::from_str(text.trim_end()).unwrap_or_else(|e| panic!("not one value ({e}): {text}"))
}

#[test]
fn under_json_a_remote_failure_is_one_error_object_with_the_outcome_as_its_result() {
    let outcome = json!({"tool": "agy", "ok": false, "error": "boom"});
    let reply = json!({
        "job": "j-2", "state": "failed", "exit": 1, "warning": "Worker runs agy 1.5.0",
        "out": format!("{outcome}\n"), "err": "noise\n",
    });
    let got = run(&["agy", "--json", "--", "x"], "worker", Ok(reply));
    assert_eq!(got.code, 1);
    assert_eq!(got.err, "", "nothing on stderr under --json");
    let v = only_json(&got.out);
    assert_eq!(v["error"]["exit"], 1);
    assert_eq!(v["error"]["code"], "failed");
    assert_eq!(v["result"], outcome);
}

#[test]
fn under_json_a_remote_job_that_ran_out_of_time_is_expired() {
    let reply = json!({"job": "j-4", "state": "failed", "exit": 124, "out": "", "err": ""});
    let got = run(&["agy", "--json", "--", "x"], "worker", Ok(reply));
    assert_eq!(got.code, 124);
    assert_eq!(got.err, "");
    let v = only_json(&got.out);
    assert_eq!(v["error"]["code"], "expired");
    assert_eq!(v["error"]["reason"], "expired");
    assert!(v.get("result").is_none());
}

#[test]
fn under_json_a_remote_success_prints_the_outcome_untouched() {
    let outcome = json!({"tool": "agy", "ok": true, "reply": "fine"});
    let reply = json!({
        "job": "j-5", "state": "done", "warning": "Worker runs agy 1.5.0",
        "exit": 0, "out": format!("{outcome}\n"), "err": "note\n",
    });
    let got = run(&["agy", "--json", "--", "x"], "worker", Ok(reply));
    assert_eq!((got.code, got.err.as_str()), (0, ""));
    assert_eq!(only_json(&got.out), outcome);
}

#[test]
fn under_json_every_stop_is_one_error_object_on_the_contract_codes() {
    let bad = run(
        &["agy", "--write", "--json", "--", "hi"],
        "worker",
        Ok(json!({})),
    );
    assert_eq!(bad.code, 2);
    assert_eq!(bad.err, "");
    assert_eq!(only_json(&bad.out)["error"]["reason"], "usage");
    assert!(bad.calls.is_empty());

    let unknown = run(&["agy", "--json", "--", "hi"], "nobody", Ok(json!({})));
    assert_eq!(unknown.code, 2);
    assert_eq!(unknown.err, "");
    assert_eq!(only_json(&unknown.out)["error"]["reason"], "bad_value");

    let silent = run(
        &["agy", "--json", "--", "x"],
        "worker",
        Err(ClientError::Timeout),
    );
    assert_eq!(silent.code, 3);
    assert_eq!(silent.err, "");
    let error = only_json(&silent.out)["error"].clone();
    assert_eq!(error["reason"], "timeout");
    assert_eq!(error["op"], ops::CLUSTER_DISPATCH);

    let offline = ClientError::Api(ApiError::new(ErrorCode::Failed, "Worker is offline"));
    let refused = run(&["agy", "--json", "--", "x"], "worker", Err(offline));
    assert_eq!((refused.code, refused.err.as_str()), (1, ""));
    assert_eq!(
        only_json(&refused.out)["error"]["message"],
        "Worker is offline"
    );

    let pending = run(
        &["agy", "--json", "--", "x"],
        "worker",
        Ok(json!({"job": "j-3", "state": "running"})),
    );
    assert_eq!((pending.code, pending.err.as_str()), (1, ""));
    assert_eq!(only_json(&pending.out)["error"]["exit"], 1);
}

#[test]
fn without_json_every_stop_ends_its_line_with_the_codes_name() {
    let unknown = run(&["agy", "--", "hi"], "nobody", Ok(json!({})));
    assert_eq!(unknown.err, "No node named nobody. (bad_request)\n");
    let silent = run(&["agy", "--", "x"], "worker", Err(ClientError::Timeout));
    assert_eq!(silent.code, 3);
    assert!(
        silent.err.trim_end().ends_with("(not_running)"),
        "{}",
        silent.err
    );
    let failed = run(
        &["agy", "--", "x"],
        "bbbbbbbbbbbbbbbb",
        Ok(json!({"job": "j-2", "state": "failed", "exit": 1, "out": "", "err": "boom\n"})),
    );
    assert_eq!(failed.code, 1);
    assert!(failed.err.starts_with("boom\n"), "{}", failed.err);
    assert!(
        failed.err.trim_end().ends_with("(failed)"),
        "{}",
        failed.err
    );
}

#[test]
fn under_json_a_remote_reply_that_is_already_an_error_object_is_not_wrapped_again() {
    let outcome = json!({"tool": "agy", "ok": false, "error": "boom"});
    let failed = Failure::failed("The job failed.").to_value_with(outcome.clone());
    let expired = Failure::new(contract::EXPIRED, "expired", "The job ran out of time.").to_value();
    let usage = Failure::usage("--folder is needed").to_value();
    for (exit, sent) in [(1, failed.clone()), (124, expired), (2, usage)] {
        let reply = json!({
            "job": "j-6", "state": "failed", "exit": exit, "out": format!("{sent}\n"), "err": "",
        });
        let got = run(&["agy", "--json", "--", "x"], "worker", Ok(reply));
        assert_eq!(got.code, exit);
        assert_eq!(got.err, "", "nothing on stderr under --json");
        assert_eq!(only_json(&got.out), sent, "exit {exit} is passed through");
    }
    let reply = json!({
        "job": "j-6", "state": "failed", "exit": 1, "out": format!("{failed}\n"), "err": "",
    });
    let got = run(&["agy", "--json", "--", "x"], "worker", Ok(reply));
    assert_eq!(only_json(&got.out)["result"], outcome);
}

#[test]
fn without_json_a_remote_failure_line_is_not_printed_twice() {
    for (exit, err) in [
        (1, "Agy failed.\nThe job failed. (failed)\n"),
        (124, "The job ran out of time. (expired)\n"),
        (2, "--folder is needed (bad_request)\n"),
    ] {
        let reply = json!({"job": "j-7", "state": "failed", "exit": exit, "out": "", "err": err});
        let got = run(&["agy", "--", "x"], "worker", Ok(reply));
        assert_eq!(got.code, exit);
        assert_eq!(got.err, err, "exit {exit}");
        assert_eq!(got.out, "");
    }
}
