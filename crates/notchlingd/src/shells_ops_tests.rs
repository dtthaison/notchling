use super::*;
use crate::consent_testkit::{agent_caller, app_caller, verified};
use notchling_local_api::ErrorCode;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const SESSION: &str = "5e5510aa-0000-4000-8000-000000000001";
const LOOP: &str = "while pgrep -f stage-build > /dev/null; do sleep 5; done";

struct Home(PathBuf);

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn launch_line(n: usize, command: &str, id: &str, at: &str) -> String {
    let call = json!({"type":"assistant","timestamp":at,"message":{"role":"assistant","content":[
        {"type":"tool_use","id":format!("toolu_{n}"),"name":"Bash","input":{"command":command,"run_in_background":true}}]}});
    let result = json!({"type":"user","timestamp":at,"message":{"role":"user","content":[
        {"type":"tool_result","tool_use_id":format!("toolu_{n}"),"is_error":false,
         "content":format!("Command running in background with ID: {id}. Output is being written to: /tmp/nl-fake/{id}.output")}]}});
    format!("{call}\n{result}\n")
}

fn stop_line(n: usize, id: &str, at: &str) -> String {
    let call = json!({"type":"assistant","timestamp":at,"message":{"role":"assistant","content":[
        {"type":"tool_use","id":format!("toolu_stop{n}"),"name":"KillShell","input":{"shell_id":id}}]}});
    let result = json!({"type":"user","timestamp":at,"message":{"role":"user","content":[
        {"type":"tool_result","tool_use_id":format!("toolu_stop{n}"),"is_error":false,"content":"killed"}]}});
    format!("{call}\n{result}\n")
}

fn home(tag: &str, transcript: &str) -> (Home, Roots) {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "nl-shells-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let projects = dir.join(".claude/projects/-work");
    std::fs::create_dir_all(&projects).unwrap();
    std::fs::write(projects.join(format!("{SESSION}.jsonl")), transcript).unwrap();
    let roots = Roots::new(&dir, &dir.join("data"));
    (Home(dir), roots)
}

fn ops_with(
    roots: Roots,
    running: &'static [&'static str],
    stopped: Arc<Mutex<Vec<String>>>,
    survive: bool,
    session: Option<&'static str>,
) -> ShellsOps {
    ShellsOps::new(roots).with_seams(
        Arc::new(move |path| {
            if running
                .iter()
                .any(|r| path.ends_with(&format!("{r}.output")))
            {
                vec![77]
            } else {
                Vec::new()
            }
        }),
        Arc::new(move |path, _| {
            stopped.lock().unwrap().push(path.to_string());
            Outcome {
                targets: 1,
                killed: 0,
                survivors: usize::from(survive),
            }
        }),
        Arc::new(|| 1_798_000_000.0),
        Some(Arc::new(move |_| match session {
            Some(s) => verified(s),
            None => AgentIdentity::shell(),
        })),
    )
}

fn transcript() -> String {
    let started = "2026-11-21T07:00:00.000Z";
    let mut text = String::new();
    text += &launch_line(1, LOOP, "bself", started);
    text += &launch_line(2, "cargo watch -x test", "bwatch", started);
    text += &launch_line(3, "sleep 600", "bdone", started);
    text += &stop_line(3, "bdone", "2026-11-21T07:05:00.000Z");
    text
}

#[test]
fn an_agent_sees_only_its_own_sessions_shells_with_state_and_stuck() {
    let (_home, roots) = home("mine", &transcript());
    let stopped = Arc::default();
    let ops = ops_with(roots, &["bself", "bwatch"], stopped, false, Some(SESSION));
    let out = ops
        .call(&agent_caller(), ops::SHELLS_MINE, &json!({}))
        .unwrap();
    let rows = out["shells"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    let by = |id: &str| rows.iter().find(|r| r["id"] == id).unwrap();
    assert_eq!(by("bself")["state"], "running");
    assert_eq!(by("bself")["stuck"], true);
    assert_eq!(by("bself")["stuckReason"], "matches_itself");
    assert_eq!(by("bwatch")["stuck"], false);
    assert!(by("bwatch")["runningSecs"].as_u64().unwrap() > 0);
    assert_eq!(by("bdone")["state"], "ended");
    assert_eq!(by("bdone")["status"], "stopped");
    assert!(by("bdone")["runningSecs"].is_null());
}

#[test]
fn a_caller_with_no_verified_session_gets_no_session() {
    let (_home, roots) = home("anon", &transcript());
    let ops = ops_with(roots, &["bself"], Arc::default(), false, None);
    for (op, args) in [
        (ops::SHELLS_MINE, json!({})),
        (ops::SHELLS_STOP, json!({"id": "bself"})),
    ] {
        let err = ops.call(&agent_caller(), op, &args).unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden, "{op}");
        assert_eq!(err.data.as_ref().unwrap()["reason"], "no_session");
    }
    let err = ops
        .call(&app_caller(), ops::SHELLS_MINE, &json!({}))
        .unwrap_err();
    assert_eq!(err.data.as_ref().unwrap()["reason"], "no_session");
}

#[test]
fn stop_ends_only_the_named_shell_of_the_callers_own_session() {
    let (_home, roots) = home("stop", &transcript());
    let stopped: Arc<Mutex<Vec<String>>> = Arc::default();
    let ops = ops_with(
        roots,
        &["bself", "bwatch"],
        stopped.clone(),
        false,
        Some(SESSION),
    );
    let out = ops
        .call(&agent_caller(), ops::SHELLS_STOP, &json!({"id": "bself"}))
        .unwrap();
    assert_eq!(
        (out["stopped"].clone(), out["state"].clone()),
        (json!(true), json!("ended"))
    );
    assert_eq!(*stopped.lock().unwrap(), ["/tmp/nl-fake/bself.output"]);

    let done = ops
        .call(&agent_caller(), ops::SHELLS_STOP, &json!({"id": "bdone"}))
        .unwrap();
    assert_eq!(done["stopped"], false);
    assert_eq!(
        stopped.lock().unwrap().len(),
        1,
        "an ended shell is not touched"
    );

    let other = ops
        .call(&agent_caller(), ops::SHELLS_STOP, &json!({"id": "bother"}))
        .unwrap_err();
    assert_eq!(other.code, ErrorCode::BadRequest);
    assert_eq!(stopped.lock().unwrap().len(), 1);

    let none = ops
        .call(&agent_caller(), ops::SHELLS_STOP, &json!({}))
        .unwrap_err();
    assert_eq!(none.code, ErrorCode::BadRequest);
}

#[test]
fn another_sessions_transcript_is_never_read() {
    let (_home, roots) = home("elsewhere", &transcript());
    let stopped: Arc<Mutex<Vec<String>>> = Arc::default();
    let ops = ops_with(
        roots,
        &["bself"],
        stopped.clone(),
        false,
        Some("another-session"),
    );
    let out = ops
        .call(&agent_caller(), ops::SHELLS_MINE, &json!({}))
        .unwrap();
    assert_eq!(out["shells"], json!([]));
    let err = ops
        .call(&agent_caller(), ops::SHELLS_STOP, &json!({"id": "bself"}))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
    assert!(stopped.lock().unwrap().is_empty());
}

#[test]
fn a_shell_that_will_not_die_is_a_failure() {
    let (_home, roots) = home("stubborn", &transcript());
    let ops = ops_with(roots, &["bself"], Arc::default(), true, Some(SESSION));
    let err = ops
        .call(&agent_caller(), ops::SHELLS_STOP, &json!({"id": "bself"}))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
}

#[test]
fn the_ops_have_the_agent_classes() {
    let ops = ops_with(
        Roots::new(
            &PathBuf::from("/nonexistent"),
            &PathBuf::from("/nonexistent"),
        ),
        &[],
        Arc::default(),
        false,
        None,
    );
    assert_eq!(ops.spec(ops::SHELLS_MINE), Some(OpSpec::AGENT_READ));
    assert_eq!(ops.spec(ops::SHELLS_STOP), Some(OpSpec::AGENT_ACTS));
    assert_eq!(ops.spec("shells.other"), None);
}
