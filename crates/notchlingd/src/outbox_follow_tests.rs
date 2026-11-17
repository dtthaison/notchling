use super::*;
use crate::outbox_courier::JobFiles;
use crate::outbox_testkit::FakeFiles;
use notchling_engine::sessions::Agent;

struct Elsewhere {
    inner: Arc<FakeFiles>,
    found: Mutex<Option<String>>,
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
        self.inner.queue(transcript)
    }
    fn process(&self, job: &str) -> Option<(u32, Option<i64>)> {
        self.inner.process(job)
    }
    fn locate(&self, _agent: &Agent) -> Option<String> {
        self.found.lock().unwrap().clone()
    }
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

fn moves_to(e: &Env, at_ms: i64, folder: &str, file: &str) {
    let path = e.dir.0.join(folder).join(file);
    let files = e.r.files.clone();
    let write: Write = Box::new(move || {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        append(&path, &[operation(at_ms, "dequeue")]);
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
    let found = Mutex::new(Some(current.to_string_lossy().into_owned()));
    let files = Arc::new(Elsewhere {
        inner: r.files.clone(),
        found,
    });
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
        "the new file is in another folder and has none of the head lines the successor search wants"
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
    moves_to(&e, 5_000, "worktree", "conv.jsonl");
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
    agent.resume_session_id = Some("new-conv".into());
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
