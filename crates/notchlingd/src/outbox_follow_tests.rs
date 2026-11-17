use super::*;
use crate::outbox_courier::JobFiles;
use crate::outbox_testkit::FakeFiles;
use notchling_engine::sessions::Agent;

struct Elsewhere {
    inner: Arc<FakeFiles>,
    found: Mutex<Option<String>>,
    asked: Mutex<Vec<String>>,
    born: AtomicUsize,
}

impl JobFiles for Elsewhere {
    fn row(&self, job: &str) -> Option<Agent> {
        self.inner.row(job)
    }
    fn state(&self, job: &str) -> Option<String> {
        self.inner.state(job)
    }
    fn status(&self, job: &str) -> Option<String> {
        self.inner.status(job)
    }
    fn restarted(&self, agent: &Agent, now_ms: i64) -> bool {
        self.inner.restarted(agent, now_ms)
    }
    fn queue(&self, transcript: &str) -> Option<QueueState> {
        self.asked.lock().unwrap().push(transcript.to_string());
        self.inner.queue(transcript)
    }
    fn process(&self, job: &str) -> Option<(u32, Option<i64>)> {
        self.inner.process(job)
    }
    fn locate(&self, _agent: &Agent) -> Option<String> {
        self.found.lock().unwrap().clone()
    }
    fn born_ms(&self, path: &str) -> Option<i64> {
        self.born.fetch_add(1, Ordering::SeqCst);
        self.inner.born_ms(path)
    }
    fn shares_name(&self, agent: &Agent, path: &str) -> bool {
        self.inner.shares_name(agent, path)
    }
}

fn elsewhere(r: &Rig, found: Option<String>) -> Arc<Elsewhere> {
    Arc::new(Elsewhere {
        inner: r.files.clone(),
        found: Mutex::new(found),
        asked: Mutex::new(Vec::new()),
        born: AtomicUsize::new(0),
    })
}

fn started_at(r: &Rig, dir: &Folder, folder: &str, file: &str) -> PathBuf {
    let path = dir.0.join(folder).join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    append(&path, &[operation(-60_000, "dequeue")]);
    let mut agent = r.files.row(IDLE).unwrap();
    agent.transcript_path = Some(path.to_string_lossy().into_owned());
    agent.name = "worker".into();
    r.files.put(IDLE, agent);
    path
}

fn env_over(files: Arc<dyn JobFiles>, r: Rig, dir: Folder, path: PathBuf) -> Env {
    let writes = Arc::new(Writes::default());
    let ticks = r.clock.clone();
    let clock: Clock = Arc::new(move || ticks.load(Ordering::SeqCst));
    let pause = Arc::new(FakePause {
        clock: r.clock.clone(),
        writes: writes.clone(),
    });
    let verifier = Arc::new(TranscriptVerifier::with_parts(
        files,
        clock.clone(),
        pause,
        r.safe.clone(),
    ));
    let postmen = Postmen::build(
        r.ops.clone(),
        Couriers::claude_only(r.courier.clone()),
        verifier.clone(),
        r.acting_switch(),
        clock,
    );
    Env {
        r,
        dir,
        path,
        writes,
        verifier,
        postmen,
    }
}

fn clear_head(ms: i64) -> Vec<String> {
    vec![
        json!({"type": "user", "timestamp": stamp(ms), "sessionId": "s-2",
            "message": {"role": "user", "content": "<command-name>/clear</command-name>\n<command-message>clear</command-message>"}})
        .to_string(),
        json!({"type": "system", "subtype": "local_command", "timestamp": stamp(ms),
            "commandRun": {"command": "clear", "args": ""}})
        .to_string(),
    ]
}

fn moves_to(e: &Env, at_ms: i64, folder: &str, file: &str) {
    moves_with(e, at_ms, folder, file, clear_head(at_ms));
}

fn moves_with(e: &Env, at_ms: i64, folder: &str, file: &str, lines: Vec<String>) {
    let path = e.dir.0.join(folder).join(file);
    let files = e.r.files.clone();
    let write: Write = Box::new(move || {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        append(&path, &lines);
        let mut agent = files.row(IDLE).unwrap();
        agent.transcript_path = Some(path.to_string_lossy().into_owned());
        files.put(IDLE, agent);
    });
    e.writes.at.lock().unwrap().push((T0 + at_ms, write));
}

#[test]
fn a_row_is_found_in_the_job_s_current_transcript_when_the_recorded_path_is_missing() {
    let r = rig();
    let dir = Folder::new("follow-missing");
    let current = started_at(&r, &dir, "worktree", "new-conv.jsonl");
    let mut agent = r.files.row(IDLE).unwrap();
    agent.transcript_path = Some(
        dir.0
            .join("cwd-folder")
            .join("new-conv.jsonl")
            .to_string_lossy()
            .into_owned(),
    );
    r.files.put(IDLE, agent);
    let files = elsewhere(&r, Some(current.to_string_lossy().into_owned()));
    let e = env_over(files, r, dir, current);
    let row = e.r.send("job-idle", "hello there");
    e.later(2_000, vec![user(2_000, "hello there")]);
    e.pass(0);
    assert_eq!(e.r.row(&row.id), None, "typed into the job, so it is Sent");
    assert_eq!(e.clock(), 2_000);
}

#[test]
fn a_clear_is_sent_when_the_job_moves_to_a_new_conversation_in_another_folder() {
    let r = rig();
    let dir = Folder::new("follow-clear");
    let old = started_at(&r, &dir, "storyboard", "old-conv.jsonl");
    let files = r.files.clone();
    let e = env_over(files, r, dir, old);
    let row = e.r.send("job-idle", "/clear");
    e.pass(0);
    assert_eq!(e.state(&row.id), Some((State::Waiting, Some(Why::Turn))));
    moves_to(&e, 5_000, "worktree", "new-conv.jsonl");
    e.pass(3_000);
    assert_eq!(
        e.r.row(&row.id),
        None,
        "the new file is in another folder, was made after the try and opens with the clear"
    );
    assert_eq!(e.clock(), 5_000);
}

#[test]
fn a_clear_is_not_sent_when_only_the_folder_of_the_same_conversation_changes() {
    let r = rig();
    let dir = Folder::new("follow-folder");
    let old = started_at(&r, &dir, "cwd-folder", "conv.jsonl");
    let files = r.files.clone();
    let e = env_over(files, r, dir, old);
    let row = e.r.send("job-idle", "/clear");
    e.pass(0);
    moves_with(
        &e,
        5_000,
        "worktree",
        "conv.jsonl",
        vec![operation(5_000, "dequeue")],
    );
    e.pass(3_000);
    assert_eq!(e.state(&row.id), Some((State::Unsure, Some(Why::NotFound))));
}

#[test]
fn the_live_lookup_prefers_the_new_conversation_to_the_one_before_it() {
    let r = rig();
    let dir = Folder::new("follow-locate");
    let projects = dir.0.join("claude").join("projects");
    let old = projects.join("-work-demo").join("old-conv.jsonl");
    let new = projects
        .join("-work-demo--claude-worktrees-music")
        .join("new-conv.jsonl");
    for path in [&old, &new] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        append(path, &[operation(0, "dequeue")]);
    }
    let roots = Roots {
        main_dir: dir.0.join("claude"),
        lanes_file: dir.0.join("lanes.json"),
    };
    let files = crate::outbox_courier::live_files(roots);
    let mut agent = r.files.row(IDLE).unwrap();
    agent.id = "old-conv".into();
    agent.cwd = "/work/demo".into();
    agent.worktree_path = Some("/work/demo/.claude/worktrees/music".into());
    agent.resume_session_id = Some("new-conv".into());
    agent.config_dir = None;
    agent.transcript_path = Some(
        projects
            .join("-work-demo")
            .join("new-conv.jsonl")
            .to_string_lossy()
            .into_owned(),
    );
    assert_eq!(
        files.locate(&agent),
        Some(new.to_string_lossy().into_owned()),
        "the recorded path is missing; the file stem names the conversation the job is in now"
    );
    agent.transcript_path = None;
    assert_eq!(
        files.locate(&agent),
        Some(new.to_string_lossy().into_owned()),
        "the resume id comes before the session id the job was started with"
    );
}

fn flips_to(e: &Env, at_ms: i64, path: PathBuf) {
    let files = e.r.files.clone();
    let write: Write = Box::new(move || {
        let mut agent = files.row(IDLE).unwrap();
        agent.transcript_path = Some(path.to_string_lossy().into_owned());
        files.put(IDLE, agent);
    });
    e.writes.at.lock().unwrap().push((T0 + at_ms, write));
}

fn made_before(e: &Env, folder: &str, file: &str, lines: Vec<String>) -> PathBuf {
    let path = e.dir.0.join(folder).join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    append(&path, &lines);
    e.r.files.born_at(&path, T0 - 3_600_000);
    path
}

#[test]
fn a_clear_is_not_sent_when_the_job_flips_to_an_older_file_that_opens_with_a_clear() {
    let r = rig();
    let dir = Folder::new("guard-older");
    let old = started_at(&r, &dir, "cwd-folder", "conv.jsonl");
    let e = env_over(r.files.clone(), r, dir, old);
    let prev = made_before(&e, "worktree", "prev.jsonl", clear_head(-3_000_000));
    let row = e.r.send("job-idle", "/clear");
    e.pass(0);
    flips_to(&e, 5_000, prev);
    e.pass(3_000);
    assert_eq!(
        e.state(&row.id),
        Some((State::Unsure, Some(Why::NotFound))),
        "a file that existed before the try cannot be the clear's new conversation"
    );
}

#[test]
fn a_clear_is_not_sent_by_a_new_file_that_does_not_open_with_a_clear() {
    let r = rig();
    let dir = Folder::new("guard-noclear");
    let old = started_at(&r, &dir, "cwd-folder", "conv.jsonl");
    let e = env_over(r.files.clone(), r, dir, old);
    let row = e.r.send("job-idle", "/clear");
    e.pass(0);
    moves_with(
        &e,
        5_000,
        "worktree",
        "restart.jsonl",
        vec![operation(5_000, "dequeue")],
    );
    e.pass(3_000);
    assert_eq!(e.state(&row.id), Some((State::Unsure, Some(Why::NotFound))));
}

#[test]
fn a_clear_is_not_sent_by_the_name_when_another_job_shares_it() {
    let r = rig();
    let dir = Folder::new("guard-name");
    let old = started_at(&r, &dir, "cwd-folder", "conv.jsonl");
    r.files.shared_name.store(true, Ordering::SeqCst);
    let e = env_over(r.files.clone(), r, dir, old.clone());
    let row = e.r.send("job-idle", "/clear");
    e.pass(0);
    let next = old.with_file_name("next.jsonl");
    let head = json!({"type": "system", "agentName": "worker", "content": "SessionStart:clear",
        "timestamp": stamp(5_000)})
    .to_string();
    let write: Write = Box::new(move || append(&next, &[head]));
    e.writes.at.lock().unwrap().push((T0 + 5_000, write));
    e.pass(3_000);
    assert_eq!(
        e.state(&row.id),
        Some((State::Unsure, Some(Why::NotFound))),
        "another job named worker in the folder may have made that file"
    );
}

#[test]
fn a_row_is_not_sent_by_its_text_in_another_file_that_existed_before_the_try() {
    let r = rig();
    let dir = Folder::new("guard-repeat");
    let first = started_at(&r, &dir, "cwd-folder", "conv.jsonl");
    let e = env_over(r.files.clone(), r, dir, first);
    let other = made_before(&e, "worktree", "other.jsonl", vec![user(-1_000, "ok")]);
    let row = e.r.send("job-idle", "ok");
    flips_to(&e, 3_000, other);
    e.pass(0);
    assert_eq!(
        e.state(&row.id),
        Some((State::Unsure, Some(Why::NotFound))),
        "the earlier ok in the other file is not this row's"
    );
}

#[test]
fn a_row_is_sent_by_its_text_in_a_file_made_after_the_try() {
    let r = rig();
    let dir = Folder::new("guard-after");
    let first = started_at(&r, &dir, "cwd-folder", "conv.jsonl");
    let e = env_over(r.files.clone(), r, dir, first);
    let row = e.r.send("job-idle", "ok");
    moves_with(&e, 3_000, "worktree", "next.jsonl", vec![user(4_000, "ok")]);
    e.pass(0);
    assert_eq!(e.r.row(&row.id), None);
}

#[test]
fn a_row_with_no_file_at_the_try_is_not_sent_by_an_older_file_found_later() {
    let r = rig();
    let dir = Folder::new("guard-nowhere");
    let mut agent = r.files.row(IDLE).unwrap();
    agent.transcript_path = None;
    r.files.put(IDLE, agent);
    let files = elsewhere(&r, None);
    let path = dir.0.join("worktree").join("late.jsonl");
    let e = env_over(files.clone(), r, dir, path);
    let older = made_before(&e, "worktree", "late.jsonl", vec![user(-1_000, "ok")]);
    let row = e.r.send("job-idle", "ok");
    let found = older.to_string_lossy().into_owned();
    let write: Write = Box::new(move || *files.found.lock().unwrap() = Some(found));
    e.writes.at.lock().unwrap().push((T0 + 3_000, write));
    e.pass(0);
    assert_eq!(e.state(&row.id), Some((State::Unsure, Some(Why::NotFound))));
}

#[test]
fn the_courier_reads_the_queue_from_the_same_file_the_verifier_counts() {
    let r = rig();
    let dir = Folder::new("guard-facts");
    let current = dir.0.join("worktree").join("new-conv.jsonl");
    std::fs::create_dir_all(current.parent().unwrap()).unwrap();
    append(&current, &[operation(0, "dequeue")]);
    let mut agent = r.files.row(IDLE).unwrap();
    agent.transcript_path = Some(
        dir.0
            .join("gone")
            .join("old.jsonl")
            .to_string_lossy()
            .into_owned(),
    );
    r.files.put(IDLE, agent);
    let files = elsewhere(&r, Some(current.to_string_lossy().into_owned()));
    let courier = crate::outbox_courier::ClaudeCourier::with_files(
        r.acting.clone(),
        files.clone(),
        Arc::new(|| T0),
        r.safe.clone(),
    );
    let row = r.send("job-idle", "hello");
    use crate::outbox_courier::Courier as _;
    courier.facts(&row, T0);
    assert_eq!(
        files.asked.lock().unwrap().clone(),
        vec![current.to_string_lossy().into_owned()],
        "the recorded path is gone, so the lookup names the file the job is in now"
    );
}

#[test]
fn the_live_lookup_never_reads_a_file_of_another_config_dir() {
    let r = rig();
    let dir = Folder::new("follow-lane");
    let main = dir.0.join("claude").join("projects");
    let lane = dir.0.join("lane").join("projects");
    let folder = "-work-demo";
    for projects in [&main, &lane] {
        let path = projects.join(folder).join("conv.jsonl");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        append(&path, &[operation(0, "dequeue")]);
    }
    std::fs::write(
        dir.0.join("lanes.json"),
        json!([{"name": "b", "path": dir.0.join("lane").to_string_lossy(),
            "accountID": "a", "email": "e"}])
        .to_string(),
    )
    .unwrap();
    let roots = Roots {
        main_dir: dir.0.join("claude"),
        lanes_file: dir.0.join("lanes.json"),
    };
    let files = crate::outbox_courier::live_files(roots);
    let mut agent = r.files.row(IDLE).unwrap();
    agent.id = "conv".into();
    agent.cwd = "/work/demo".into();
    agent.worktree_path = None;
    agent.resume_session_id = None;
    agent.transcript_path = None;
    agent.config_dir = None;
    assert_eq!(
        files.locate(&agent),
        Some(
            main.join(folder)
                .join("conv.jsonl")
                .to_string_lossy()
                .into_owned()
        )
    );
    agent.config_dir = Some(dir.0.join("lane").to_string_lossy().into_owned());
    assert_eq!(
        files.locate(&agent),
        Some(
            lane.join(folder)
                .join("conv.jsonl")
                .to_string_lossy()
                .into_owned()
        ),
        "a job of a lane reads its own lane's folder, not the main one"
    );
    agent.config_dir = Some("/lanes/unknown".into());
    assert_eq!(files.locate(&agent), None);
}

#[test]
fn a_sending_row_at_start_is_counted_in_the_conversation_the_job_resumed_into() {
    let s = start();
    let resumed = s.dir.0.join("projects/-work-demo/resume-conv.jsonl");
    std::fs::create_dir_all(resumed.parent().unwrap()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    append(&resumed, &[user(1_000, "hi")]);
    let state = s.dir.0.join(format!("claude/jobs/{JOB}/state.json"));
    let mut job: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&state).unwrap()).unwrap();
    job["resumeSessionId"] = json!("resume-conv");
    std::fs::write(&state, job.to_string()).unwrap();
    let row = s.sending(JOB, Tool::Claude, "hi", Some(Seen::default()));
    s.ops.load(T0 + 60_000);
    assert_eq!(
        s.state(&row.id),
        None,
        "the start reads the file the job is writing, not the old one its link names"
    );
}

#[test]
fn a_late_look_and_the_queue_watch_never_compare_counts_across_files() {
    let r = rig();
    let dir = Folder::new("guard-late");
    let first = started_at(&r, &dir, "cwd-folder", "conv.jsonl");
    let e = env_over(r.files.clone(), r, dir, first);
    let other = made_before(&e, "worktree", "other.jsonl", vec![user(-1_000, "ok")]);
    let row = e.r.send("job-idle", "ok");
    e.r.ops.store().take(&row.id, T0).unwrap().unwrap();
    let row = e.r.row(&row.id).unwrap();
    let before = e.verifier.seen(&row).seen;
    let mut agent = e.r.files.row(IDLE).unwrap();
    agent.transcript_path = Some(other.to_string_lossy().into_owned());
    e.r.files.put(IDLE, agent);
    assert!(!e.verifier.found_late(&row, before));
    assert_eq!(
        e.verifier.watch(&row, before, T0 + 5_000),
        QueuedVerdict::Stay,
        "the ok in the other file is not this row's"
    );
}

#[path = "outbox_proof_tests.rs"]
mod proof;
