use super::*;
use crate::consent_testkit::{agent_caller, app_caller, verified};
use notchling_local_api::ErrorCode;
use std::collections::HashMap;
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

fn running_map(running: &'static [&'static str], paths: &[String]) -> HashMap<String, Vec<i32>> {
    paths
        .iter()
        .map(|path| {
            let live = running
                .iter()
                .any(|r| path.ends_with(&format!("{r}.output")));
            (path.clone(), if live { vec![77] } else { Vec::new() })
        })
        .collect()
}

fn ops_with(
    roots: Roots,
    running: &'static [&'static str],
    stopped: Arc<Mutex<Vec<String>>>,
    survive: bool,
    session: Option<&'static str>,
) -> ShellsOps {
    ShellsOps::new(roots)
        .with_seams(
            Arc::new(move |paths| running_map(running, paths)),
            Arc::new(move |request, _| {
                stopped.lock().unwrap().push(request.path.to_string());
                Ok(Outcome {
                    targets: 1,
                    killed: 0,
                    survivors: usize::from(survive),
                    left: Vec::new(),
                })
            }),
            Arc::new(|| 1_798_000_000.0),
            Some(Arc::new(move |_| match session {
                Some(s) => verified(s),
                None => AgentIdentity::shell(),
            })),
        )
        .with_owner(Arc::new(|_, _| Some(4242)))
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
fn a_stop_that_leaves_writers_running_says_so() {
    let (_home, roots) = home("left", &transcript());
    let ops = ShellsOps::new(roots)
        .with_seams(
            Arc::new(|paths| running_map(&["bself"], paths)),
            Arc::new(|_, _| {
                Ok(Outcome {
                    targets: 1,
                    killed: 1,
                    survivors: 0,
                    left: vec![801, 802],
                })
            }),
            Arc::new(|| 1_798_000_000.0),
            Some(Arc::new(|_| verified(SESSION))),
        )
        .with_owner(Arc::new(|_, _| Some(4242)));
    let out = ops
        .call(&agent_caller(), ops::SHELLS_STOP, &json!({"id": "bself"}))
        .unwrap();
    assert_eq!(out["stopped"], false);
    assert_eq!(out["state"], "running");
    assert_eq!(out["left"], json!([801, 802]));
    assert_eq!(out["reason"], "some_left");
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

#[test]
fn mine_asks_for_every_unfinished_shells_writers_in_one_batch() {
    let (_home, roots) = home("batch", &transcript());
    let calls: Arc<Mutex<Vec<Vec<String>>>> = Arc::default();
    let seen = calls.clone();
    let ops = ShellsOps::new(roots).with_seams(
        Arc::new(move |paths| {
            seen.lock().unwrap().push(paths.to_vec());
            running_map(&["bself"], paths)
        }),
        Arc::new(|_, _| Ok(Outcome::default())),
        Arc::new(|| 1_798_000_000.0),
        Some(Arc::new(|_| verified(SESSION))),
    );
    let out = ops
        .call(&agent_caller(), ops::SHELLS_MINE, &json!({}))
        .unwrap();
    assert_eq!(out["shells"].as_array().unwrap().len(), 3);
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "one lookup for all rows");
    assert_eq!(
        calls[0],
        ["/tmp/nl-fake/bself.output", "/tmp/nl-fake/bwatch.output"],
        "the finished shell is not looked at"
    );
}

#[test]
fn stop_names_the_session_the_shell_its_file_and_the_owner_process() {
    let (_home, roots) = home("request", &transcript());
    let got: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = got.clone();
    let ops = ShellsOps::new(roots)
        .with_seams(
            Arc::new(|paths| running_map(&["bself"], paths)),
            Arc::new(move |request, _| {
                seen.lock().unwrap().push(format!(
                    "{} {} {} {:?} {}",
                    request.session, request.id, request.path, request.owner, request.orphans
                ));
                Ok(Outcome {
                    targets: 1,
                    killed: 0,
                    survivors: 0,
                    left: Vec::new(),
                })
            }),
            Arc::new(|| 1_798_000_000.0),
            Some(Arc::new(|_| verified(SESSION))),
        )
        .with_owner(Arc::new(|_, session| {
            (session == SESSION).then_some(31_337)
        }));
    ops.call(&agent_caller(), ops::SHELLS_STOP, &json!({"id": "bself"}))
        .unwrap();
    assert_eq!(
        *got.lock().unwrap(),
        [format!(
            "{SESSION} bself /tmp/nl-fake/bself.output Some(31337) false"
        )]
    );
}

#[test]
fn a_refused_stop_is_forbidden_with_the_reason() {
    for (refusal, reason) in [
        (Refusal::NotAShellOutput, "not_a_shell_output"),
        (Refusal::OwnerUnknown, "owner_unknown"),
    ] {
        let (_home, roots) = home("refused", &transcript());
        let ops = ShellsOps::new(roots).with_seams(
            Arc::new(|paths| running_map(&["bself"], paths)),
            Arc::new(move |_, _| Err(refusal)),
            Arc::new(|| 1_798_000_000.0),
            Some(Arc::new(|_| verified(SESSION))),
        );
        let err = ops
            .call(&agent_caller(), ops::SHELLS_STOP, &json!({"id": "bself"}))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
        assert_eq!(err.data.as_ref().unwrap()["reason"], reason);
    }
}

#[test]
fn the_real_stopper_refuses_a_path_outside_claudes_tasks_folder() {
    let (_home, roots) = home("real", &transcript());
    let ops = ShellsOps::new(roots)
        .with_seams(
            Arc::new(|paths| running_map(&["bself"], paths)),
            Arc::new(shells_stop::stop_shell),
            Arc::new(|| 1_798_000_000.0),
            Some(Arc::new(|_| verified(SESSION))),
        )
        .with_owner(Arc::new(|_, _| Some(4242)));
    let err = ops
        .call(&agent_caller(), ops::SHELLS_STOP, &json!({"id": "bself"}))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert_eq!(err.data.as_ref().unwrap()["reason"], "not_a_shell_output");
}
