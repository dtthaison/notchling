use super::*;
use crate::outbox_anchor::{count_row, remember_anchor, Anchor};
use notchling_engine::sessions::AgentTarget;
use std::time::{SystemTime, UNIX_EPOCH};

fn taken_row(e: &Env, text: &str, at_ms: i64) -> Row {
    let row = e.r.send("job-idle", text);
    e.r.ops.store().take(&row.id, T0 + at_ms).unwrap().unwrap();
    e.r.row(&row.id).unwrap()
}

#[test]
fn a_clear_made_just_before_the_try_never_proves_a_later_clear() {
    let r = rig();
    let dir = Folder::new("proof-recent");
    let old = started_at(&r, &dir, "cwd-folder", "conv.jsonl");
    let e = env_over(r.files.clone(), r, dir, old);
    let row = e.r.send("job-idle", "/clear");
    e.pass(0);
    let recent = made_before(&e, "worktree", "recent.jsonl", clear_head(2_000));
    e.r.files.born_at(&recent, T0 + 2_000);
    flips_to(&e, 5_000, recent);
    e.pass(3_000);
    assert_eq!(
        e.state(&row.id),
        Some((State::Unsure, Some(Why::NotFound))),
        "a clear file made 1 s before the try, inside the 2 s slack, is the earlier clear's"
    );
}

#[test]
fn a_clear_is_counted_once() {
    let r = rig();
    let dir = Folder::new("proof-once");
    let old = started_at(&r, &dir, "cwd-folder", "conv.jsonl");
    let e = env_over(r.files.clone(), r, dir, old);
    let row = taken_row(&e, "/clear", 0);
    let fresh = e.dir.0.join("worktree").join("fresh.jsonl");
    std::fs::create_dir_all(fresh.parent().unwrap()).unwrap();
    append(&fresh, &clear_head(500));
    e.r.files.born_at(&fresh, T0 + 500);
    let agent = e.r.files.row(IDLE).unwrap();
    let path = fresh.to_string_lossy().into_owned();
    let seen = count_row(e.r.files.as_ref(), &agent, &path, &row, 0).unwrap();
    assert_eq!(
        seen.taken, 1,
        "the command line and the clear head are one clear, not two"
    );
}

fn roots_with(dir: &Folder) -> Roots {
    Roots {
        main_dir: dir.0.join("claude"),
        lanes_file: dir.0.join("none/lanes.json"),
    }
}

fn write_job(dir: &Folder, id: &str, state: serde_json::Value) {
    let file = dir.0.join(format!("claude/jobs/{id}/state.json"));
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, state.to_string()).unwrap();
}

fn write_session(dir: &Folder, session: serde_json::Value) {
    let file = dir.0.join("claude/sessions/live.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, session.to_string()).unwrap();
}

fn mine(r: &Rig) -> Agent {
    let mut agent = r.files.row(IDLE).unwrap();
    agent.name = "worker".into();
    agent.target = AgentTarget::Job("aaaa1111".into());
    agent
}

#[test]
fn a_name_is_shared_by_any_job_session_or_folder_that_could_make_the_file() {
    let r = rig();
    let dir = Folder::new("proof-names");
    let files = crate::outbox_courier::live_files(roots_with(&dir));
    let path = "/h/.claude/projects/-work-demo/conv.jsonl";
    let agent = mine(&r);
    assert!(!files.shares_name(&agent, path), "alone");
    write_job(
        &dir,
        "aaaa1111",
        json!({"name": "worker", "cwd": "/work/demo", "state": "working"}),
    );
    assert!(
        !files.shares_name(&agent, path),
        "the job itself is not a sibling"
    );

    write_job(
        &dir,
        "bbbb2222",
        json!({"name": "worker", "cwd": "/work/demo", "state": "stopped"}),
    );
    assert!(
        files.shares_name(&agent, path),
        "a stopped sibling can be resumed"
    );

    write_job(
        &dir,
        "bbbb2222",
        json!({"name": "worker", "cwd": "/work/other", "worktreePath": "/work/demo", "state": "working"}),
    );
    assert!(
        files.shares_name(&agent, path),
        "its cwd differs but its transcripts land in the same project folder"
    );

    write_job(
        &dir,
        "bbbb2222",
        json!({"name": "worker", "cwd": "/work/other", "state": "working"}),
    );
    assert!(
        !files.shares_name(&agent, path),
        "another folder, no collision"
    );

    write_job(
        &dir,
        "bbbb2222",
        json!({"name": "worker", "cwd": "/work/other", "state": "working",
            "linkScanPath": "/h/.claude/projects/-work-demo/other.jsonl"}),
    );
    assert!(
        files.shares_name(&agent, path),
        "its link names this folder"
    );

    write_job(
        &dir,
        "bbbb2222",
        json!({"name": "someone", "cwd": "/work/demo", "state": "working"}),
    );
    assert!(!files.shares_name(&agent, path), "another name");

    let tmp = "/h/.claude/projects/-private-tmp-app/conv.jsonl";
    write_job(
        &dir,
        "bbbb2222",
        json!({"name": "worker", "cwd": "/tmp/app", "state": "working"}),
    );
    assert!(
        files.shares_name(&agent, tmp),
        "/tmp/app is /private/tmp/app"
    );
}

#[test]
fn a_name_is_shared_by_a_live_session_that_is_not_this_job() {
    let r = rig();
    let dir = Folder::new("proof-sessions");
    let files = crate::outbox_courier::live_files(roots_with(&dir));
    let path = "/h/.claude/projects/-work-demo/conv.jsonl";
    let agent = mine(&r);
    let pid = std::process::id();
    write_session(
        &dir,
        json!({"pid": pid, "name": "worker", "cwd": "/work/demo", "sessionId": "s1"}),
    );
    assert!(
        files.shares_name(&agent, path),
        "a claude --name worker in the folder"
    );
    write_session(
        &dir,
        json!({"pid": pid, "name": "worker", "cwd": "/work/demo", "jobId": "aaaa1111"}),
    );
    assert!(!files.shares_name(&agent, path), "this job's own session");
    write_session(
        &dir,
        json!({"pid": 999_999_999u64, "name": "worker", "cwd": "/work/demo"}),
    );
    assert!(
        !files.shares_name(&agent, path),
        "a dead session is not running a clear"
    );
}

fn real_now_ms() -> i64 {
    let at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    i64::try_from(at.as_millis()).unwrap()
}

#[test]
fn a_sending_row_at_start_is_not_sent_by_a_line_just_before_the_try() {
    let s = start();
    append(&s.transcript, &[user(-1_000, "hi")]);
    let row = s.sending(JOB, Tool::Claude, "hi", Some(Seen::default()));
    s.ops.load(T0 + 60_000);
    assert_eq!(
        s.state(&row.id),
        Some((State::Unsure, Some(Why::Restart))),
        "with no anchor the count starts at the try, not 2 s before it"
    );
}

#[test]
fn a_sending_row_with_an_anchor_is_not_sent_by_another_file_that_existed_before_the_try() {
    let s = start();
    let attempt = real_now_ms() + 3_600_000;
    let stamped = |ms: i64| {
        json!({"type": "user", "message": {"role": "user", "content": "hi"}, "uuid": "u",
            "timestamp": iso8601(attempt + ms), "sessionId": "s-1"})
        .to_string()
    };
    append(&s.transcript, &[stamped(1_000)]);
    let store = s.ops.store();
    let new = New {
        session: JOB.into(),
        tool: Tool::Claude,
        text: "hi".into(),
        options: None,
        via: Via::Chat,
        clear_box: false,
    };
    let row = store.insert(&new, attempt - 1_000).unwrap();
    store.take(&row.id, attempt).unwrap().unwrap();
    store.set_seen(&row.id, Seen::default()).unwrap();
    remember_anchor(&row.id, Anchor::File("/elsewhere/first.jsonl".into()));
    s.ops.load(attempt + 60_000);
    assert_eq!(
        s.state(&row.id),
        Some((State::Unsure, Some(Why::Restart))),
        "the file the job is in now existed before the try and is not the one the baseline counted"
    );
    crate::outbox_anchor::forget_anchor(&row.id);
}

#[test]
fn the_watch_sees_a_clear_successor_made_while_the_old_file_is_unchanged() {
    let r = rig();
    let dir = Folder::new("proof-watch");
    let old = started_at(&r, &dir, "cwd-folder", "conv.jsonl");
    let e = env_over(r.files.clone(), r, dir, old.clone());
    let row = taken_row(&e, "/clear", 0);
    let before = e.verifier.seen(&row).unwrap_or_default();
    assert_eq!(
        e.verifier.watch(&row, before, T0 + 1_000),
        QueuedVerdict::Stay
    );
    let next = old.with_file_name("next.jsonl");
    append(
        &next,
        &[
            json!({"type": "system", "agentName": "worker", "content": "SessionStart:clear",
            "timestamp": stamp(1_500)})
            .to_string(),
        ],
    );
    assert_eq!(
        e.verifier.watch(&row, before, T0 + 2_000),
        QueuedVerdict::Sent,
        "the old file did not change, but the folder did"
    );
}
