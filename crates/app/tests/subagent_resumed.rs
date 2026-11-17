use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use notchling_app::chat::data::DataRoot;
use notchling_app::chat::loader::Loader;
use notchling_engine::bridge::net::files::now_ms;
use notchling_engine::transcript::subagents::Status;

const MAIN: &str = include_str!("../../engine/tests/fixtures/background-job/main.jsonl");
const RESUMED: &str =
    include_str!("../../engine/tests/fixtures/background-job/agent-resumed.jsonl");
const HANDED_BACK: &str =
    include_str!("../../engine/tests/fixtures/background-job/agent-handed-back.jsonl");
const META: &str = r#"{"agentType":"general-purpose","description":"Research film pacing","toolUseId":"toolu_launch01","requestShape":"background","model":"sonnet"}"#;
const JOB: &str = "job-film-1";
const RESUME_ID: &str = "5e5510aa-0000-4000-8000-000000000001";
const AGENT: &str = "a0b1c2d3e4f5a6b7c";

struct World {
    dir: PathBuf,
}

impl World {
    fn new(agent_file: &str) -> World {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "notchling-subagent-resumed-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let project = dir.join("claude/projects/-work-film");
        let subagents = project.join(format!("{RESUME_ID}/subagents"));
        std::fs::create_dir_all(&subagents).expect("folder");
        std::fs::write(project.join(format!("{RESUME_ID}.jsonl")), MAIN).expect("transcript");
        std::fs::write(subagents.join(format!("agent-{AGENT}.meta.json")), META).expect("meta");
        std::fs::write(subagents.join(format!("agent-{AGENT}.jsonl")), agent_file).expect("agent");
        let job = dir.join("claude/jobs/9f3883da");
        std::fs::create_dir_all(&job).expect("job folder");
        std::fs::write(
            job.join("state.json"),
            format!(
                r#"{{"state":"working","tempo":"active","sessionId":"{JOB}","resumeSessionId":"{RESUME_ID}","name":"film research","cwd":"/work/film","updatedAt":"2026-11-17T03:25:00.983Z","linkScanPath":"claude/projects/-work-film/{RESUME_ID}.jsonl","template":"bg"}}"#
            ),
        )
        .expect("state");
        World { dir }
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn a_background_job_whose_agent_was_resumed_shows_it_running_in_the_details_panel() {
    let world = World::new(RESUMED);
    let mut loader = Loader::new(DataRoot::fixture(&world.dir));
    let loaded = loader.load(Some(JOB), now_ms());

    assert_eq!(loaded.selected.as_deref(), Some(JOB));
    assert_eq!(loaded.subagents.len(), 1, "{:?}", loaded.subagents);
    assert_eq!(loaded.subagents[0].status, Status::Running);
    assert_eq!(loaded.subagent_rows.len(), 1);
    assert!(loaded.subagent_rows[0].running);
    assert_eq!(loaded.subagent_counts.running, 1);
}

#[test]
fn a_background_job_whose_agent_reported_and_was_not_resumed_shows_it_completed() {
    let world = World::new(HANDED_BACK);
    let main: String = MAIN.lines().take(4).map(|l| format!("{l}\n")).collect();
    std::fs::write(
        world
            .dir
            .join(format!("claude/projects/-work-film/{RESUME_ID}.jsonl")),
        main,
    )
    .expect("rewrite");
    let mut loader = Loader::new(DataRoot::fixture(&world.dir));
    let loaded = loader.load(Some(JOB), now_ms());

    assert_eq!(loaded.subagents.len(), 1, "{:?}", loaded.subagents);
    assert_eq!(loaded.subagents[0].status, Status::Completed);
    assert_eq!(loaded.subagent_counts.running, 0);
}
