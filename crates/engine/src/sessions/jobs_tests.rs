use super::agent::{AgentGroup, AgentTarget};
use super::jobs::*;
use super::lanes::Roots;
use super::testutil::TempDir;
use serde_json::{json, Value};

fn none(_: &str) -> Option<f64> {
    None
}

fn job(state: &str, extra: Value) -> Value {
    let mut v = json!({ "state": state, "sessionId": "sess-1", "name": "fix-hook", "cwd": "/Users/me/app" });
    if let (Some(base), Some(more)) = (v.as_object_mut(), extra.as_object()) {
        for (k, val) in more {
            base.insert(k.clone(), val.clone());
        }
    }
    v
}

#[test]
fn live_copy_first() {
    let old = json!({"createdAt": "2026-10-28T08:20:11.786Z", "updatedAt": "2026-11-02T08:21:06.159Z", "state": "stopped"});
    let moved = json!({"createdAt": "2026-11-02T08:21:08.279Z", "updatedAt": "2026-11-03T08:00:00.000Z", "state": "working"});
    let lane = Some("/lanes/b".to_string());
    assert_eq!(
        newest_first(&[(None, old.clone()), (lane.clone(), moved)]),
        vec![lane.clone(), None]
    );
    let woken = json!({"createdAt": "2026-10-28T08:20:11.786Z", "updatedAt": "2026-11-04T03:22:20.219Z", "state": "running"});
    let left = json!({"createdAt": "2026-11-02T08:21:08.279Z", "updatedAt": "2026-11-04T03:15:40.662Z", "state": "stopped"});
    assert_eq!(
        newest_first(&[(None, woken), (lane.clone(), left)]),
        vec![None, lane]
    );
}

#[test]
fn both_stopped_newest_update_first() {
    let a = json!({"updatedAt": "2026-11-02T08:21:06.159Z", "state": "stopped"});
    let b = json!({"updatedAt": "2026-11-04T03:15:40.662Z", "state": "stopped"});
    let lane = Some("/lanes/b".to_string());
    assert_eq!(
        newest_first(&[(None, a), (lane.clone(), b.clone())]),
        vec![lane.clone(), None]
    );
    assert_eq!(
        newest_first(&[(None, json!({})), (lane.clone(), b)]),
        vec![None, lane]
    );
}

#[test]
fn created_at_never_decides() {
    let a = json!({"createdAt": "2026-10-01T00:00:00Z", "updatedAt": "2026-11-05T00:00:00Z", "state": "working"});
    let b = json!({"createdAt": "2026-11-01T00:00:00Z", "updatedAt": "2026-11-02T00:00:00Z", "state": "working"});
    assert_eq!(
        newest_first(&[(Some("b".into()), b), (None, a)]),
        vec![None, Some("b".into())]
    );
}

#[test]
fn live_status_wins_over_state_json() {
    let live = json!({"status": "idle", "jobId": "ab12cd34"});
    let a = agent_from_job(&job("working", json!({})), "ab12cd34", Some(&live), &none);
    assert_eq!((a.group, a.label.as_str()), (AgentGroup::Completed, "Idle"));
    let busy = json!({"status": "busy"});
    let b = agent_from_job(&job("blocked", json!({})), "ab12cd34", Some(&busy), &none);
    assert_eq!(
        (b.group, b.label.as_str()),
        (AgentGroup::Working, "Working")
    );
}

#[test]
fn job_states_map_to_groups() {
    let cases = [
        ("working", AgentGroup::Working, "Working"),
        ("blocked", AgentGroup::NeedsInput, "Needs input"),
        ("running", AgentGroup::Completed, "Idle"),
        ("stopped", AgentGroup::Completed, "Stopped"),
        ("done", AgentGroup::Completed, "Done"),
        ("", AgentGroup::Completed, "Idle"),
    ];
    for (state, group, label) in cases {
        let a = agent_from_job(&job(state, json!({})), "ab", None, &none);
        assert_eq!(
            (a.group, a.label.as_str()),
            (group, label),
            "state {state:?}"
        );
    }
    let tempo = job("idle", json!({"tempo": "blocked"}));
    assert_eq!(
        agent_from_job(&tempo, "ab", None, &none).group,
        AgentGroup::NeedsInput
    );
    let active = job("idle", json!({"tempo": "active"}));
    assert_eq!(
        agent_from_job(&active, "ab", None, &none).group,
        AgentGroup::Working
    );
    let waiting = json!({"status": "waiting"});
    assert_eq!(
        agent_from_job(&job("working", json!({})), "ab", Some(&waiting), &none).group,
        AgentGroup::NeedsInput
    );
}

#[test]
fn needs_input_shows_needs_over_detail() {
    let j = job(
        "blocked",
        json!({"needs": "answer: Which db? (a · b)", "detail": "editing"}),
    );
    let a = agent_from_job(&j, "ab", None, &none);
    assert_eq!(a.detail.as_deref(), Some("answer: Which db? (a · b)"));
    let w = job("working", json!({"needs": "x", "detail": "editing"}));
    assert_eq!(
        agent_from_job(&w, "ab", None, &none).detail.as_deref(),
        Some("editing")
    );
}

#[test]
fn since_is_the_later_of_update_and_live_status() {
    let j = job("working", json!({"updatedAt": "1970-01-01T00:00:10.000Z"}));
    assert_eq!(agent_from_job(&j, "ab", None, &none).since_ms, 10_000);
    let live = json!({"status": "busy", "statusUpdatedAt": 20_000.0});
    assert_eq!(
        agent_from_job(&j, "ab", Some(&live), &none).since_ms,
        20_000
    );
    let early = json!({"status": "busy", "statusUpdatedAt": 5_000.0});
    assert_eq!(
        agent_from_job(&j, "ab", Some(&early), &none).since_ms,
        10_000
    );
    assert_eq!(
        agent_from_job(&job("working", json!({})), "ab", None, &none).since_ms,
        0
    );
}

#[test]
fn exec_template_is_a_command_job() {
    let j = job(
        "working",
        json!({"template": "exec", "intent": "cd /p && make > run.log 2>&1"}),
    );
    let a = agent_from_job(&j, "ab", None, &none);
    assert_eq!(a.command.as_deref(), Some("cd /p && make > run.log 2>&1"));
    assert!(!a.can_reply());
    let no_intent = job("working", json!({"template": "exec"}));
    assert_eq!(
        agent_from_job(&no_intent, "ab", None, &none)
            .command
            .as_deref(),
        Some("")
    );
    assert_eq!(
        agent_from_job(&job("working", json!({})), "ab", None, &none).command,
        None
    );
}

#[test]
fn job_ids_names_and_target() {
    let a = agent_from_job(&job("working", json!({})), "ab12cd34", None, &none);
    assert_eq!(
        (a.id.as_str(), a.name.as_str(), a.project.as_str()),
        ("sess-1", "fix-hook", "app")
    );
    assert_eq!(a.target, AgentTarget::Job("ab12cd34".into()));
    assert_eq!(a.host, "Background");
    let bare = agent_from_job(&json!({"state": "working"}), "ab12cd34", None, &none);
    assert_eq!(
        (bare.id.as_str(), bare.name.as_str()),
        ("ab12cd34", "ab12cd34")
    );
}

#[test]
fn transcript_follows_the_session_into_its_worktree() {
    let start = "/h/.claude/projects/-Users-me-app/sess-1.jsonl";
    let moved = "/h/.claude/projects/-Users-me-app--claude-worktrees-music/sess-1.jsonl";
    let j = job(
        "working",
        json!({"linkScanPath": start, "worktreePath": "/Users/me/app/.claude/worktrees/music"}),
    );
    let at = |p: &str| (p == moved).then_some(5.0);
    assert_eq!(
        agent_from_job(&j, "ab", None, &at)
            .transcript_path
            .as_deref(),
        Some(moved)
    );
    assert_eq!(
        agent_from_job(&j, "ab", None, &none)
            .transcript_path
            .as_deref(),
        Some(start)
    );
}

#[test]
fn equal_modification_times_keep_the_recorded_path() {
    let start = "/h/.claude/projects/-Users-me-app/sess-1.jsonl";
    let moved = "/h/.claude/projects/-Users-me-app--claude-worktrees-music/sess-1.jsonl";
    let j = job(
        "working",
        json!({"linkScanPath": start, "worktreePath": "/Users/me/app/.claude/worktrees/music"}),
    );
    let same = |_: &str| Some(5.0);
    let found = agent_from_job(&j, "ab", None, &same).transcript_path;
    assert_eq!(found.as_deref(), Some(start), "not {moved}");
}

#[test]
fn resume_session_id_is_read() {
    let j = job("working", json!({"resumeSessionId": "old-conv"}));
    let a = agent_from_job(&j, "ab", None, &none);
    assert_eq!(a.resume_session_id.as_deref(), Some("old-conv"));
}

#[test]
fn sdk_cli_sessions_are_headless() {
    let s = json!({"sessionId": "s", "pid": 4242, "status": "busy", "cwd": "/w/app", "entrypoint": "sdk-cli"});
    let host = |_: u32| Some("iTerm".to_string());
    let a = agent_from_session(&s, &host).expect("session");
    assert!(a.headless);
    assert_eq!(a.host, "claude -p");
    assert_eq!(a.target, AgentTarget::Process(4242));
    assert_eq!(
        (a.group, a.label.as_str()),
        (AgentGroup::Working, "Working")
    );
    let tty = json!({"sessionId": "s", "pid": 7, "status": "waiting", "cwd": "/w/app"});
    let b = agent_from_session(&tty, &host).expect("session");
    assert!(!b.headless);
    assert_eq!(
        (b.host.as_str(), b.group, b.name.as_str()),
        ("iTerm", AgentGroup::NeedsInput, "app")
    );
    assert_eq!(agent_from_session(&json!({"pid": 7}), &host), None);
    assert_eq!(agent_from_session(&json!({"sessionId": "s"}), &host), None);
    let fallback = agent_from_session(&tty, &|_| None).expect("session");
    assert_eq!(fallback.host, "Terminal");
}

#[test]
fn reads_jobs_and_live_sessions_from_a_folder() {
    let tmp = TempDir::new("jobs");
    tmp.write("c/jobs/aa11bb22/state.json", r#"{"state":"working"}"#);
    tmp.write("c/jobs/cc33dd44/state.json", "{ broken");
    tmp.write("c/jobs/ee55ff66/other.txt", "x");
    tmp.write("c/sessions/100.json", r#"{"sessionId":"s1","pid":100}"#);
    tmp.write("c/sessions/200.json", r#"{"sessionId":"s2","pid":200}"#);
    tmp.write("c/sessions/notes.txt", "nope");
    let jobs = read_jobs(&tmp.path("c"));
    assert_eq!(jobs.keys().collect::<Vec<_>>(), vec!["aa11bb22"]);
    let sessions = read_sessions(&tmp.path("c"), &|pid| pid == 100);
    assert_eq!(sessions.len(), 1);
    assert!(read_jobs(&tmp.path("missing")).is_empty());
}

#[test]
fn finds_the_current_lane_of_a_moved_job() {
    let tmp = TempDir::new("jobdirs");
    let lane = tmp.path("lanes/b");
    tmp.write(
        "data/lanes.json",
        &format!(
            r#"[{{"name":"b","path":"{}","accountID":"a","email":"e"}}]"#,
            lane.display()
        ),
    );
    tmp.write(
        "home/.claude/jobs/ab12cd34/state.json",
        r#"{"state":"stopped","updatedAt":"2026-11-02T08:21:06.159Z"}"#,
    );
    tmp.write(
        "lanes/b/jobs/ab12cd34/state.json",
        r#"{"state":"working","updatedAt":"2026-11-03T08:00:00.000Z"}"#,
    );
    let roots = Roots::new(&tmp.path("home"), &tmp.path("data"));
    let dirs = job_dirs(&roots, "ab12cd34");
    assert_eq!(dirs, vec![Some(lane.display().to_string()), None]);
    assert_eq!(
        config_dir_for_job(&roots, "ab12cd34"),
        Some(lane.display().to_string())
    );
    assert_eq!(config_dir_for_job(&roots, "ffffffff"), None);
}

#[test]
fn launch_flags_drop_resume_and_background() {
    let j = json!({"respawnFlags": ["--name", "x", "--resume", "abc", "--bg", "-r", "def", "--model", "opus", "--background"]});
    assert_eq!(
        launch_flags(&j, "x"),
        vec!["--name", "x", "--model", "opus"]
    );
    assert_eq!(
        launch_flags(&json!({}), "fix-hook"),
        vec!["--name", "fix-hook"]
    );
}

#[test]
fn resume_flags_only_when_no_job_to_continue() {
    let flags = vec!["--name".to_string(), "x".to_string()];
    assert_eq!(
        resume_arguments("conv", false, &flags),
        vec!["--name", "x", "--bg", "--resume", "conv"]
    );
    assert_eq!(
        resume_arguments("conv", true, &flags),
        vec!["--bg", "--resume", "conv"]
    );
}
