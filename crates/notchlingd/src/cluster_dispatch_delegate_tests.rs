use super::{args_of, check, run_remote};
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
    assert_eq!(got.err, "No node named nobody.\n");
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
    assert_eq!(got.err, "boom\n");
}

#[test]
fn a_refusal_from_the_cluster_is_printed_and_exit_one() {
    let refused = ClientError::Api(ApiError::new(ErrorCode::Failed, "Worker is offline"));
    let got = run(&["agy", "--", "x"], "worker", Err(refused));
    assert_eq!(got.code, 1);
    assert_eq!(got.err, "Worker is offline\n");
    assert_eq!(got.out, "");
    let silent = run(&["agy", "--", "x"], "worker", Err(ClientError::Timeout));
    assert_eq!(silent.code, 2);
}

#[test]
fn a_reply_without_an_exit_code_says_where_the_job_stands() {
    let reply = json!({"job": "j-3", "state": "running"});
    let got = run(&["agy", "--", "x"], "worker", Ok(reply));
    assert_eq!(got.code, 1);
    assert_eq!(got.err, "The job j-3 is running.\n");
}
