use super::*;
use crate::delegate::test_support::{rig, Rig};
use crate::tools::acp::fake::FakeSpawner as AcpFake;
use crate::tools::codex::fake::FakeRun;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

const STARTED: &str = r#"{"type":"thread.started","thread_id":"019e-s1"}"#;
const MESSAGE: &str =
    r#"{"type":"item.completed","item":{"type":"agent_message","text":"Looks fine."}}"#;
const DONE: &str = r#"{"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":5}}"#;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

struct Ran {
    code: i32,
    out: String,
    err: String,
}

fn delegate(rig: &Rig, args: &[&str], depth: Option<&str>) -> Ran {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let ctx = Context {
        cwd: &rig.folder,
        home: Some(&rig.root),
        depth,
    };
    let code = run(&words(args), &ctx, rig.deps.clone(), &mut out, &mut err);
    Ran {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

#[test]
fn a_codex_job_prints_the_header_the_session_and_the_reply() {
    let rig = rig(AcpFake::new());
    rig.codex.push(FakeRun::ok(&[STARTED, MESSAGE, DONE]));
    let ran = delegate(
        &rig,
        &[
            "codex", "--model", "m1", "--effort", "HIGH", "--", "review", "it",
        ],
        None,
    );
    assert_eq!(ran.code, 0);
    let mut lines = ran.out.lines();
    let head = lines.next().unwrap();
    assert!(
        head.starts_with("Codex (m1, high): done in ") && head.ends_with("s, 15 tokens"),
        "{head}"
    );
    assert_eq!(
        lines.collect::<Vec<_>>(),
        ["Session: 019e-s1", "", "Looks fine."]
    );
    assert_eq!(ran.err, "");
    // The folder defaults to the current one; read-only, so no --write.
    let spec = rig.codex.specs().remove(0);
    assert_eq!(spec.cwd, rig.folder.to_string_lossy());
    assert!(spec
        .args
        .contains(&"sandbox_mode=\"read-only\"".to_string()));
    assert_eq!(spec.args.last().map(String::as_str), Some("review it"));
}

#[test]
fn json_has_the_same_shape_as_the_swift_command() {
    let rig = rig(AcpFake::new());
    rig.codex.push(FakeRun::ok(&[STARTED, MESSAGE, DONE]));
    let ran = delegate(&rig, &["codex", "--json", "x"], None);
    assert_eq!(ran.out.lines().count(), 1);
    let v: Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(v["tool"], "codex");
    assert_eq!(v["ok"], true);
    assert_eq!(v["reply"], "Looks fine.");
    assert_eq!(v["session"], "019e-s1");
    assert_eq!(v["tokens"], 15);
    assert_eq!(v["media"], json!([]));
    assert_eq!(v["declined"], json!([]));
    assert!(v.get("timedOut").is_none() && v.get("error").is_none());
}

#[test]
fn exit_codes_are_0_1_and_124() {
    let rig = rig(AcpFake::new());
    rig.codex.push(FakeRun::ok(&[STARTED, MESSAGE]));
    assert_eq!(delegate(&rig, &["codex", "x"], None).code, 0);

    rig.codex
        .push(FakeRun::ok(&[r#"{"type":"error","message":"nope"}"#]).exiting(1));
    let failed = delegate(&rig, &["codex", "x"], None);
    assert_eq!(failed.code, 1);
    assert!(failed.out.starts_with("Codex: failed in "));
    assert!(failed.out.contains("\nError: nope"));

    rig.codex.push(FakeRun::held(&[STARTED]));
    let slow = delegate(&rig, &["codex", "--timeout", "0.2", "--json", "x"], None);
    assert_eq!(slow.code, 124);
    let v: Value = serde_json::from_str(&slow.out).unwrap();
    assert_eq!(
        (v["timedOut"].as_bool(), v["ok"].as_bool()),
        (Some(true), Some(false))
    );
}

#[test]
fn a_command_line_that_cannot_run_exits_2_and_starts_nothing() {
    let rig = rig(AcpFake::new());
    for (args, message) in [
        (vec!["claude", "x"], "Name the tool first: codex or agy."),
        (vec!["codex"], "No prompt given."),
        (
            vec!["codex", "--timeout", "0", "x"],
            "--timeout takes seconds.",
        ),
        (
            vec!["codex", "--folder", "/no/such/folder", "x"],
            "No folder at /no/such/folder.",
        ),
    ] {
        let ran = delegate(&rig, &args, None);
        assert_eq!(ran.code, 2, "{args:?}");
        assert!(ran.err.starts_with(message), "{args:?}: {}", ran.err);
        assert_eq!(ran.out, "");
    }
    assert!(rig.codex.specs().is_empty() && rig.agy.spawned().is_empty());
}

#[test]
fn a_job_that_was_itself_delegated_twice_is_refused() {
    let rig = rig(AcpFake::new());
    let ran = delegate(&rig, &["codex", "x"], Some("2"));
    assert_eq!(ran.code, 2);
    assert_eq!(
        ran.err,
        "This job was itself delegated 2 times; do it yourself instead of delegating again.\n"
    );
    assert!(rig.codex.specs().is_empty());

    rig.codex.push(FakeRun::ok(&[MESSAGE]));
    assert_eq!(delegate(&rig, &["codex", "x"], Some("1")).code, 0);
    let env = rig.codex.specs().remove(0).env;
    assert!(
        env.contains(&("NOTCHLING_DELEGATE_DEPTH".into(), Some("2".into()))),
        "the tool's own processes are one level deeper: {env:?}"
    );
}

#[test]
fn generated_media_is_listed_and_copied_to_out_without_overwriting() {
    let rig = rig(AcpFake::new());
    let session = rig.codex_media().join("019e-s1");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::write(session.join("cup.png"), "img").unwrap();
    let out = rig.root.join("assets");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("cup.png"), "already here").unwrap();
    rig.codex.push(FakeRun::ok(&[STARTED, MESSAGE]));
    let ran = delegate(
        &rig,
        &["codex", "--out", out.to_str().unwrap(), "draw a cup"],
        None,
    );
    assert_eq!(ran.code, 0);
    let generated = session.join("cup.png");
    let copied = out.join("cup-2.png");
    assert!(
        ran.out.contains(&format!(
            "Generated:\n  {}\nCopied to:\n  {}\n",
            generated.display(),
            copied.display()
        )),
        "{}",
        ran.out
    );
    assert_eq!(std::fs::read_to_string(&copied).unwrap(), "img");
    assert_eq!(
        std::fs::read_to_string(out.join("cup.png")).unwrap(),
        "already here"
    );
}

#[test]
fn a_copy_that_fails_fails_the_run() {
    let rig = rig(AcpFake::new());
    let session = rig.codex_media().join("019e-s1");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::write(session.join("cup.png"), "img").unwrap();
    let not_a_folder = rig.root.join("file");
    std::fs::write(&not_a_folder, "x").unwrap();
    rig.codex.push(FakeRun::ok(&[STARTED, MESSAGE]));
    let ran = delegate(
        &rig,
        &["codex", "--out", not_a_folder.to_str().unwrap(), "draw"],
        None,
    );
    assert_eq!(ran.code, 1);
    assert!(ran.out.contains("Error: Couldn't copy to "), "{}", ran.out);
}

#[test]
fn an_agy_job_waits_for_the_turn_and_prints_what_it_declined() {
    let responder: crate::tools::acp::fake::Responder = Arc::new(
        |m: &Value, h: &crate::tools::acp::fake::FakeHandle| {
            let id = m["id"].clone();
            let reply =
                |result: Value| h.push(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
            match m.get("method").and_then(Value::as_str) {
                Some("initialize") => reply(json!({})),
                Some("session/new") => reply(json!({"sessionId": "s9", "configOptions": []})),
                Some("session/prompt") => {
                    h.push(&json!({"jsonrpc": "2.0", "id": "perm", "method": "session/request_permission",
                        "params": {"toolCall": {"kind": "execute", "title": "Run make"},
                            "options": [{"optionId": "no", "kind": "reject_once"}]}}));
                    h.push(&json!({"jsonrpc": "2.0", "method": "session/update", "params": {"update":
                        {"sessionUpdate": "agent_message_chunk", "content": {"text": "Reviewed."}}}}));
                    reply(json!({"stopReason": "end_turn", "usage": {"totalTokens": 99}}));
                }
                _ => {}
            }
        },
    );
    let rig = rig(AcpFake::with_responder(responder));
    let ran = delegate(&rig, &["agy", "--", "review", "the", "diff"], None);
    assert_eq!(ran.code, 0, "{} {}", ran.out, ran.err);
    assert!(
        ran.out.starts_with("Antigravity CLI: done in "),
        "{}",
        ran.out
    );
    assert!(ran.out.contains(", 99 tokens\nSession: s9\nDeclined (not allowed in a delegated run):\n  Run make\n\nReviewed."));
    let prompt = &rig.agy.spawned()[0].requests("session/prompt")[0];
    assert_eq!(prompt["params"]["prompt"][0]["text"], "review the diff");
}

const CACHE: &str = r#"{"models":[
  {"slug":"gpt-a","display_name":"A","priority":1,"visibility":"list","default_reasoning_level":"medium",
   "supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"}]},
  {"slug":"gpt-b","priority":2,"visibility":"list"}]}"#;

fn listing(rig: &Rig, args: &[&str]) -> String {
    let mut out = Vec::new();
    assert_eq!(models_listing(&words(args), &rig.deps, &mut out), 0);
    let text = String::from_utf8(out).unwrap();
    assert!(text.ends_with('\n'));
    text.trim_end().to_string()
}

#[test]
fn delegate_models_lists_both_tools() {
    let mut rig = rig(AcpFake::new());
    std::fs::create_dir_all(&rig.deps.codex_home).unwrap();
    std::fs::write(rig.deps.codex_home.join("models_cache.json"), CACHE).unwrap();
    rig.deps.capture = Arc::new(|program, args| {
        assert_eq!((program, args), (Path::new("/fake/agy"), &["models"][..]));
        Some("Fetching available models...\ngemini-4-argon-medium\tM\ngemini-4-argon-high\tH\nflash\tF\n".into())
    });
    assert_eq!(
        listing(&rig, &[]),
        "codex (--model <slug> --effort <level>):\n  gpt-a  efforts: low, medium (default medium)\n  gpt-b  efforts: \nagy (--model <model> --effort low|medium|high):\n  gemini-4-argon  efforts: medium, high\n  flash"
    );
    assert_eq!(
        listing(&rig, &["agy"]),
        "agy (--model <model> --effort low|medium|high):\n  gemini-4-argon  efforts: medium, high\n  flash"
    );
    assert!(listing(&rig, &["codex"]).starts_with("codex ("));
    assert!(!listing(&rig, &["Codex"]).contains("agy ("));
}

#[test]
fn delegate_models_says_so_when_a_tool_has_nothing_to_list() {
    let rig = rig(AcpFake::new());
    assert_eq!(
        listing(&rig, &[]),
        "codex (--model <slug> --effort <level>):\n  (Codex hasn't cached its model list yet; run codex once)\nagy (--model <model> --effort low|medium|high):\n  (couldn't run agy models)"
    );
}

#[test]
fn a_timeout_longer_than_the_run_does_not_cut_it_short() {
    let rig = rig(AcpFake::new());
    rig.codex.push(FakeRun::ok(&[MESSAGE]));
    let began = std::time::Instant::now();
    let ran = delegate(&rig, &["codex", "--timeout", "600", "x"], None);
    assert_eq!(ran.code, 0);
    assert!(began.elapsed() < Duration::from_secs(5));
}
