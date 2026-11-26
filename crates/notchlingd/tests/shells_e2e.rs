#![cfg(unix)]

mod common;

use common::{audit, home, notchling, scratch, Daemon};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const SESSION: &str = "5e5510aa-0000-4000-8000-000000000001";
const STUCK_COMMAND: &str = "while pgrep -f shells-e2e-stuck-marker >/dev/null; do sleep 1; done";
const WATCH_COMMAND: &str = "sleep 300";
const STUCK_PROCESS: &str = "while :; do sleep 1; done";

fn launch_lines(n: usize, command: &str, id: &str, output: &Path, at: &str) -> String {
    let call = json!({"type":"assistant","timestamp":at,"message":{"role":"assistant","content":[
        {"type":"tool_use","id":format!("toolu_{n}"),"name":"Bash","input":{"command":command,"run_in_background":true}}]}});
    let result = json!({"type":"user","timestamp":at,"message":{"role":"user","content":[
        {"type":"tool_result","tool_use_id":format!("toolu_{n}"),"is_error":false,
         "content":format!("Command running in background with ID: {id}. Output is being written to: {}", output.display())}]}});
    format!("{call}\n{result}\n")
}

fn fake_shell(script: &str, output: &Path) -> Child {
    let file = std::fs::File::create(output).unwrap();
    Command::new("sh")
        .args(["-c", script])
        .stdin(Stdio::null())
        .stdout(file)
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .unwrap()
}

#[test]
fn shells_helper_runs_commands_as_the_session_process() {
    let Ok(bin) = std::env::var("NL_SHELLS_E2E_BIN") else {
        return;
    };
    let mut go = String::new();
    std::io::stdin().lock().read_line(&mut go).unwrap();
    let steps: Vec<Vec<String>> =
        serde_json::from_str(&std::env::var("NL_SHELLS_E2E_STEPS").unwrap()).unwrap();
    let spawn: Vec<(String, String)> =
        serde_json::from_str(&std::env::var("NL_SHELLS_E2E_SPAWN").unwrap()).unwrap();
    let kids: Vec<Child> = spawn
        .iter()
        .map(|(script, output)| fake_shell(script, Path::new(output)))
        .collect();
    std::thread::sleep(Duration::from_millis(500));
    let pids: Vec<u32> = kids.iter().map(Child::id).collect();
    println!("PIDS {}", json!(pids));
    for step in steps {
        let out = Command::new(&bin)
            .args(&step)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        println!(
            "STEP {}",
            json!({"code": out.status.code(), "out": String::from_utf8_lossy(&out.stdout),
                   "err": String::from_utf8_lossy(&out.stderr)})
        );
    }
}

fn as_session(
    root: &Path,
    home: &Path,
    spawn: &[(&str, &Path)],
    steps: &[&[&str]],
) -> (Vec<u32>, Vec<Value>) {
    let claude_dir = root.join(".local/share/claude/versions/1");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let claude = claude_dir.join("claude");
    std::fs::copy(std::env::current_exe().unwrap(), &claude).unwrap();
    let mut parent = common::spawn_retrying(
        Command::new(&claude)
            .args([
                "--exact",
                "shells_helper_runs_commands_as_the_session_process",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("NL_SHELLS_E2E_BIN", env!("CARGO_BIN_EXE_notchling"))
            .env("NL_SHELLS_E2E_STEPS", serde_json::to_string(steps).unwrap())
            .env(
                "NL_SHELLS_E2E_SPAWN",
                serde_json::to_string(
                    &spawn
                        .iter()
                        .map(|(script, output)| (*script, output.to_string_lossy()))
                        .collect::<Vec<_>>(),
                )
                .unwrap(),
            )
            .env("NOTCHLING_DATA_ROOT", root)
            .env("HOME", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()),
    );
    let sessions = root.join("claude/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(
        sessions.join(format!("{}.json", parent.id())),
        json!({"sessionId": SESSION}).to_string(),
    )
    .unwrap();
    writeln!(parent.stdin.take().unwrap(), "go").unwrap();
    let reader = BufReader::new(parent.stdout.take().unwrap());
    let lines: Vec<String> = reader.lines().map_while(Result::ok).collect();
    let pids: Vec<u32> = lines
        .iter()
        .find_map(|line| line.split_once("PIDS ").map(|(_, text)| text.to_owned()))
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let steps: Vec<Value> = lines
        .iter()
        .filter_map(|line| line.split_once("STEP ").map(|(_, text)| text.to_owned()))
        .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
        .collect();
    let _ = parent.wait();
    (pids, steps)
}

struct Cleanup(std::path::PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn running(pid: u32) -> bool {
    Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .map(|out| {
            let stat = String::from_utf8_lossy(&out.stdout);
            !stat.trim().is_empty() && !stat.trim().starts_with('Z')
        })
        .unwrap_or(false)
}

fn kill_pid(pid: u32) {
    let _ = Command::new("kill").arg(pid.to_string()).output();
}

fn parsed(step: &Value) -> Value {
    serde_json::from_str(step["out"].as_str().unwrap_or("")).unwrap_or(Value::Null)
}

#[test]
fn an_agent_lists_and_stops_its_own_stuck_shell_and_nothing_else() {
    let dir = scratch("shells-e2e");
    let root = dir.0.join("root");
    let home = home(&dir.0);
    let uid = std::fs::metadata(&dir.0).unwrap().uid();
    let project = std::env::temp_dir()
        .join(format!("claude-{uid}"))
        .join(format!("nl-shells-e2e-{}", std::process::id()));
    let _cleanup = Cleanup(project.clone());
    let fake = project.join(SESSION).join("tasks");
    std::fs::create_dir_all(&fake).unwrap();
    let (stuck_out, watch_out, stranger_out) = (
        fake.join("bstuck.output"),
        fake.join("bwatch.output"),
        fake.join("bstranger.output"),
    );
    let projects = root.join("claude/projects/-work");
    std::fs::create_dir_all(&projects).unwrap();
    let long_ago = "2020-01-01T00:00:00.000Z";
    std::fs::write(
        projects.join(format!("{SESSION}.jsonl")),
        launch_lines(1, STUCK_COMMAND, "bstuck", &stuck_out, long_ago)
            + &launch_lines(2, WATCH_COMMAND, "bwatch", &watch_out, long_ago)
            + &launch_lines(3, WATCH_COMMAND, "bstranger", &stranger_out, long_ago),
    )
    .unwrap();
    let mut stranger = fake_shell(WATCH_COMMAND, &stranger_out);
    std::thread::sleep(Duration::from_millis(500));
    let _daemon = Daemon::start(&root, &home);

    let (code, out, _) = notchling(&root, &home, &["shells", "--json"]);
    assert_eq!(code, 4, "{out}");
    let value: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["error"]["reason"], "no_session");
    let (code, out, _) = notchling(&root, &home, &["shells", "stop", "bstuck", "--json"]);
    assert_eq!(code, 4, "{out}");
    assert!(
        stranger.try_wait().unwrap().is_none(),
        "an unverified caller stops nothing"
    );

    let (pids, steps) = as_session(
        &root,
        &home,
        &[(STUCK_PROCESS, &stuck_out), (WATCH_COMMAND, &watch_out)],
        &[
            &["shells", "--json"],
            &["shells", "stop", "bstuck", "--json"],
            &["shells", "stop", "bnope", "--json"],
            &["shells", "--json"],
            &["shells"],
            &["shells", "stop", "bstranger", "--json"],
            &["shells", "--stuck", "--json"],
        ],
    );
    assert_eq!(steps.len(), 7, "{steps:?}");
    assert_eq!(pids.len(), 2, "{pids:?}");
    let (stuck_pid, watch_pid) = (pids[0], pids[1]);

    assert_eq!(steps[0]["code"], 0, "{:?}", steps[0]);
    let first = parsed(&steps[0]);
    let rows = first["shells"].as_array().unwrap();
    let by = |rows: &Vec<Value>, id: &str| rows.iter().find(|r| r["id"] == id).cloned().unwrap();
    let (a, b) = (by(rows, "bstuck"), by(rows, "bwatch"));
    assert_eq!(
        (a["state"].clone(), a["stuck"].clone()),
        (json!("running"), json!(true))
    );
    assert_eq!(a["stuckReason"], "matches_itself");
    assert_eq!(
        (b["state"].clone(), b["stuck"].clone()),
        (json!("running"), json!(false))
    );
    assert!(a["runningSecs"].as_u64().unwrap() > 1_000_000);

    assert_eq!(steps[1]["code"], 0, "{:?}", steps[1]);
    assert_eq!(parsed(&steps[1])["stopped"], true);
    assert!(
        !running(stuck_pid),
        "the stuck loop and its process group ended"
    );
    assert!(
        running(watch_pid),
        "another shell of the session is left running"
    );

    assert_eq!(steps[2]["code"], 4, "{:?}", steps[2]);

    let after = parsed(&steps[3]);
    let rows = after["shells"].as_array().unwrap();
    assert_eq!(by(rows, "bstuck")["state"], "ended");
    assert_eq!(by(rows, "bwatch")["state"], "running");

    let text = steps[4]["out"].as_str().unwrap();
    assert!(text.contains("bwatch  running"), "{text}");
    assert!(text.contains("bstuck  ended"), "{text}");

    assert_eq!(steps[5]["code"], 1, "{:?}", steps[5]);
    assert_eq!(parsed(&steps[5])["result"]["stopped"], false);
    assert_eq!(parsed(&steps[5])["error"]["reason"], "some_left");
    assert!(
        stranger.try_wait().unwrap().is_none(),
        "a process outside the session's Claude Code is never signalled"
    );

    assert_eq!(steps[6]["code"], 0, "{:?}", steps[6]);
    assert!(
        parsed(&steps[6])["shells"].as_array().unwrap().is_empty(),
        "nothing left is stuck: {:?}",
        steps[6]
    );

    let acts: Vec<(String, String)> = audit::wait_until(&root, |all| {
        all.iter().filter(|l| l["kind"] == "agent.called").count() >= 4
    })
    .iter()
    .filter(|l| l["kind"] == "agent.called")
    .map(|l| {
        (
            l["f"]["op"].as_str().unwrap_or_default().to_string(),
            l["f"]["result"].as_str().unwrap_or_default().to_string(),
        )
    })
    .collect();
    assert_eq!(
        acts.iter().filter(|(op, _)| op == "shells.stop").count(),
        4,
        "{acts:?}"
    );
    assert!(acts.contains(&("shells.stop".to_string(), "ok".to_string())));
    assert!(
        acts.iter()
            .all(|(op, _)| op != "shells.mine" && op != "shells.stuck"),
        "reads are not recorded"
    );

    kill_pid(watch_pid);
    let _ = stranger.kill();
    let _ = stranger.wait();
}
