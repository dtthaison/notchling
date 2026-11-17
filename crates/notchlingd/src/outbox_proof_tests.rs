use super::*;
use crate::outbox_anchor::{count_row, message_of};
use notchling_engine::outbox::Anchor;
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
    let seen = count_row(
        e.r.files.as_ref(),
        &agent,
        &path,
        &row,
        &message_of(&row),
        0,
    )
    .unwrap();
    assert_eq!(
        seen.taken, 1,
        "the command line and the clear head are one clear, not two"
    );
}

fn in_projects(dir: &Folder, folder: &str, file: &str) -> String {
    dir.0
        .join("claude/projects")
        .join(folder)
        .join(file)
        .to_string_lossy()
        .into_owned()
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
    let path = &in_projects(&dir, "-work-demo", "conv.jsonl");
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
            "linkScanPath": in_projects(&dir, "-work-demo", "other.jsonl")}),
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

    let tmp = &in_projects(&dir, "-private-tmp-app", "conv.jsonl");
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
    let path = &in_projects(&dir, "-work-demo", "conv.jsonl");
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
        files.shares_name(&agent, path),
        "a session that cleared and exited is gone, but its file is still the folder's newest"
    );
    write_session(
        &dir,
        json!({"pid": 999_999_999u64, "name": "worker", "cwd": "/work/other",
            "worktreePath": "/work/demo"}),
    );
    assert!(
        files.shares_name(&agent, path),
        "its worktree names the folder"
    );
    write_session(
        &dir,
        json!({"pid": pid, "name": "worker", "cwd": "/work/other", "sessionId": "sess-x"}),
    );
    assert!(
        !files.shares_name(&agent, path),
        "another folder and no conversation of its own here"
    );
    let own = dir.0.join("claude/projects/-work-demo/sess-x.jsonl");
    std::fs::create_dir_all(own.parent().unwrap()).unwrap();
    std::fs::write(own, "").unwrap();
    assert!(
        files.shares_name(&agent, path),
        "its own conversation file is in the folder, whatever its cwd says"
    );
    write_session(
        &dir,
        json!({"pid": pid, "name": "someone", "cwd": "/work/demo", "sessionId": "sess-x"}),
    );
    assert!(!files.shares_name(&agent, path), "another name");
}

#[test]
fn a_name_is_only_looked_up_in_the_config_dir_that_owns_the_file() {
    let r = rig();
    let dir = Folder::new("proof-owner");
    let lane = dir.0.join("lane");
    std::fs::write(
        dir.0.join("lanes.json"),
        json!([{"name": "b", "path": lane.to_string_lossy(), "accountID": "a", "email": "e"}])
            .to_string(),
    )
    .unwrap();
    let roots = Roots {
        main_dir: dir.0.join("claude"),
        lanes_file: dir.0.join("lanes.json"),
    };
    let files = crate::outbox_courier::live_files(roots);
    let agent = mine(&r);
    let path = in_projects(&dir, "-work-demo", "conv.jsonl");
    let sibling = json!({"name": "worker", "cwd": "/work/demo", "state": "working"});
    let in_lane = lane.join("jobs/bbbb2222/state.json");
    std::fs::create_dir_all(in_lane.parent().unwrap()).unwrap();
    std::fs::write(&in_lane, sibling.to_string()).unwrap();
    assert!(
        !files.shares_name(&agent, &path),
        "a sibling of another config dir cannot make a file of this one"
    );
    let lane_path = lane
        .join("projects/-work-demo/conv.jsonl")
        .to_string_lossy()
        .into_owned();
    assert!(
        files.shares_name(&agent, &lane_path),
        "the lane's own folder does see it"
    );
    assert!(
        files.shares_name(&agent, "/elsewhere/projects/-work-demo/conv.jsonl"),
        "a file no config dir owns cannot be tied to this job"
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
    append(&s.transcript, &[stamped(attempt, 100, "hi")]);
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
    let first = Anchor {
        file: Some("/elsewhere/first.jsonl".into()),
        slack_ms: 2_000,
    };
    store
        .set_baseline(&row.id, Seen::default(), Some(&first))
        .unwrap();
    s.ops.load(attempt + 60_000);
    assert_eq!(
        s.state(&row.id),
        Some((State::Unsure, Some(Why::Restart))),
        "the file the job is in now existed before the try and is not the one the baseline counted"
    );
}

#[test]
fn the_watch_sees_a_clear_successor_made_while_the_old_file_is_unchanged() {
    let r = rig();
    let dir = Folder::new("proof-watch");
    let old = started_at(&r, &dir, "cwd-folder", "conv.jsonl");
    let e = env_over(r.files.clone(), r, dir, old.clone());
    let row = taken_row(&e, "/clear", 0);
    let before = e.verifier.seen(&row).seen;
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

fn precise(ms: i64) -> String {
    let whole = iso8601(ms);
    format!("{}.{:03}Z", &whole[..whole.len() - 1], ms.rem_euclid(1000))
}

fn stamped(attempt: i64, ms: i64, text: &str) -> String {
    json!({"type": "user", "message": {"role": "user", "content": text}, "uuid": "u",
        "timestamp": precise(attempt + ms), "sessionId": "s-1"})
    .to_string()
}

fn taken_at(s: &Start, attempt: i64, text: &str) -> Row {
    let new = New {
        session: JOB.into(),
        tool: Tool::Claude,
        text: text.into(),
        options: None,
        via: Via::Chat,
        clear_box: false,
    };
    let store = s.ops.store();
    let row = store.insert(&new, attempt - 1_000).unwrap();
    store.take(&row.id, attempt).unwrap().unwrap();
    store.get(&row.id).unwrap().unwrap()
}

#[test]
fn a_restarted_row_counts_with_the_slack_and_file_its_baseline_was_taken_with() {
    let s = start();
    let attempt = real_now_ms() + 3_600_000;
    append(&s.transcript, &[stamped(attempt, -1_000, "ok")]);
    let row = taken_at(&s, attempt, "ok");
    let verifier = TranscriptVerifier::new(roots_with(&s.dir), Arc::new(SafeModes::default()));
    let based = verifier.seen(&row);
    assert_eq!(
        based.seen.taken, 1,
        "the earlier ok is inside the 2 s slack"
    );
    s.ops
        .store()
        .set_baseline(&row.id, based.seen, based.anchor.as_ref())
        .unwrap();
    drop(verifier);
    append(&s.transcript, &[stamped(attempt, 5_000, "ok")]);
    s.ops.load(attempt + 60_000);
    assert_eq!(
        s.state(&row.id),
        None,
        "after a restart the same file counts the same way, so the second ok is the take"
    );
}

#[test]
fn an_old_row_with_no_anchor_is_not_sent_from_a_file_born_before_the_try() {
    let s = start();
    let attempt = real_now_ms() + 3_600_000;
    append(&s.transcript, &[stamped(attempt, 100, "ok")]);
    let row = taken_at(&s, attempt, "ok");
    s.ops.store().set_seen(&row.id, Seen::default()).unwrap();
    s.ops.load(attempt + 60_000);
    assert_eq!(
        s.state(&row.id),
        Some((State::Unsure, Some(Why::Restart))),
        "nothing ties the file to the baseline, and it was there before the try"
    );
}

#[test]
fn an_old_row_with_no_anchor_is_sent_from_a_file_made_after_the_try() {
    let s = start();
    let resumed = s.dir.0.join("projects/-work-demo/later.jsonl");
    std::fs::create_dir_all(resumed.parent().unwrap()).unwrap();
    let attempt = real_now_ms() - 60_000;
    append(&resumed, &[stamped(attempt, 61_000, "ok")]);
    let job = s.dir.0.join(format!("claude/jobs/{JOB}/state.json"));
    let mut state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&job).unwrap()).unwrap();
    state["resumeSessionId"] = json!("later");
    std::fs::write(&job, state.to_string()).unwrap();
    let row = taken_at(&s, attempt, "ok");
    s.ops.store().set_seen(&row.id, Seen::default()).unwrap();
    s.ops.load(attempt + 90_000);
    assert_eq!(s.state(&row.id), None);
}

#[test]
fn the_postman_stores_the_file_and_slack_its_baseline_was_taken_with() {
    let r = rig();
    let dir = Folder::new("proof-stored");
    let old = started_at(&r, &dir, "cwd-folder", "conv.jsonl");
    let e = env_over(r.files.clone(), r, dir, old.clone());
    let row = e.r.send("job-idle", "hello there");
    e.pass(0);
    let stored = e.r.row(&row.id).unwrap();
    assert_eq!(
        stored.anchor,
        Some(Anchor {
            file: Some(old.to_string_lossy().into_owned()),
            slack_ms: crate::outbox_verify::SINCE_BEFORE_MS,
        }),
        "the row, not the process, remembers the file its baseline was counted in"
    );
}

#[test]
fn the_watch_does_not_count_a_clear_again_while_nothing_changed() {
    let r = rig();
    let dir = Folder::new("proof-watch-quiet");
    let old = started_at(&r, &dir, "cwd-folder", "conv.jsonl");
    let files = elsewhere(&r, None);
    let e = env_over(files.clone(), r, dir, old);
    let row = taken_row(&e, "/clear", 0);
    let before = Seen::default();
    assert_eq!(
        e.verifier.watch(&row, before, T0 + 1_000),
        QueuedVerdict::Stay
    );
    let counted = files.born.load(Ordering::SeqCst);
    assert!(counted > 0, "the first look counts");
    for second in 2..6 {
        assert_eq!(
            e.verifier.watch(&row, before, T0 + second * 1_000),
            QueuedVerdict::Stay
        );
    }
    assert_eq!(
        files.born.load(Ordering::SeqCst),
        counted,
        "an unchanged file and folder are not parsed again every second"
    );
}

#[test]
fn on_equal_dates_the_worktree_s_file_wins_over_the_cwd_s() {
    let r = rig();
    let dir = Folder::new("proof-tie");
    let projects = dir.0.join("claude").join("projects");
    let in_cwd = projects.join("-work-cwd").join("conv.jsonl");
    let in_tree = projects.join("-work-tree").join("conv.jsonl");
    let at = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
    for path in [&in_cwd, &in_tree] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        append(path, &[operation(0, "dequeue")]);
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(at)
            .unwrap();
    }
    let files = crate::outbox_courier::live_files(roots_with(&dir));
    let mut agent = r.files.row(IDLE).unwrap();
    agent.id = "conv".into();
    agent.cwd = "/work/cwd".into();
    agent.worktree_path = Some("/work/tree".into());
    agent.resume_session_id = None;
    agent.transcript_path = None;
    agent.config_dir = None;
    assert_eq!(
        files.locate(&agent),
        Some(in_tree.to_string_lossy().into_owned()),
        "the worktree is where the job works, so it wins a tie with the cwd it was started in"
    );
}
