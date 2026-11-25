use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use notchling_app::chat::data::DataRoot;
use notchling_app::chat::inspector::inspector_with_fan;
use notchling_app::chat::loader::Loader;
use notchling_app::chat::shell_list::ShellPage;
use notchling_engine::bridge::net::files::now_ms;
use notchling_engine::transcript::subagents::Status;

const MAIN: &str = include_str!("../../engine/tests/fixtures/background-job/main.jsonl");
const HANDED_BACK: &str =
    include_str!("../../engine/tests/fixtures/background-job/agent-handed-back.jsonl");
const META: &str = r#"{"agentType":"general-purpose","description":"Research film pacing","toolUseId":"toolu_launch01","requestShape":"background","model":"sonnet"}"#;
const JOB: &str = "job-film-1";
const SHORT: &str = "9f3883da";
const RESUME_ID: &str = "5e5510aa-0000-4000-8000-000000000001";
const AGENT: &str = "a0b1c2d3e4f5a6b7c";

struct World {
    dir: PathBuf,
}

impl World {
    fn new(fan: Option<&str>) -> World {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "notchling-job-fan-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let project = dir.join("claude/projects/-work-film");
        let subagents = project.join(format!("{RESUME_ID}/subagents"));
        std::fs::create_dir_all(&subagents).expect("folder");
        let main: String = MAIN.lines().take(4).map(|l| format!("{l}\n")).collect();
        std::fs::write(project.join(format!("{RESUME_ID}.jsonl")), main).expect("transcript");
        std::fs::write(subagents.join(format!("agent-{AGENT}.meta.json")), META).expect("meta");
        std::fs::write(subagents.join(format!("agent-{AGENT}.jsonl")), HANDED_BACK).expect("agent");
        let job = dir.join(format!("claude/jobs/{SHORT}"));
        std::fs::create_dir_all(&job).expect("job folder");
        let fan = fan.map_or(String::new(), |fan| format!(r#""fan":{fan},"#));
        std::fs::write(
            job.join("state.json"),
            format!(
                r#"{{"state":"working","tempo":"active",{fan}"sessionId":"{JOB}","resumeSessionId":"{RESUME_ID}","name":"film research","cwd":"/work/film","updatedAt":"2026-11-17T03:25:00.983Z","linkScanPath":"claude/projects/-work-film/{RESUME_ID}.jsonl","template":"bg"}}"#
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

fn fan() -> String {
    format!(
        r#"[
        {{"id":"bsub000001","kind":"shell","label":"sleep 900","startedAt":1794000000000}},
        {{"id":"{AGENT}","kind":"agent","label":"Research film pacing","startedAt":1794000100000}},
        {{"id":"bsub000002","kind":"shell","label":"make test","startedAt":1794000200000,"doneAt":1794000300000}}
    ]"#
    )
}

#[test]
fn a_job_shows_the_shells_and_running_agents_of_its_own_state_file() {
    let world = World::new(Some(&fan()));
    let mut loader = Loader::new(DataRoot::fixture(&world.dir));
    let loaded = loader.load(Some(JOB), now_ms());

    assert_eq!(loaded.selected.as_deref(), Some(JOB));
    assert_eq!(loaded.fan_shells.len(), 1, "{:?}", loaded.fan_shells);
    assert_eq!(loaded.fan_shells[0].id, "bsub000001");
    assert_eq!(loaded.fan_shells[0].command, "sleep 900");

    assert_eq!(loaded.subagents.len(), 1, "{:?}", loaded.subagents);
    assert_eq!(loaded.subagents[0].status, Status::Running);
    assert!(loaded.subagent_rows[0].running);
    assert_eq!(loaded.subagent_counts.running, 1);

    let mut page = ShellPage::default();
    let listing = page.show_with_fan(
        Some(JOB),
        &loaded.conversation,
        &loaded.fan_shells,
        0,
        &|_| None,
    );
    let i = inspector_with_fan(
        &loaded.conversation,
        &loaded.fan_shells,
        &loaded.subagent_rows,
        loaded.subagent_counts,
        Some(&listing),
        &|_| Vec::new(),
    );
    assert_eq!(i.shells.len(), 1);
    assert!(i.shells[0].running);
    assert_eq!(i.shells[0].title, "sleep 900");
}

#[test]
fn a_job_whose_state_file_has_no_fan_is_shown_as_the_transcript_says() {
    let world = World::new(None);
    let mut loader = Loader::new(DataRoot::fixture(&world.dir));
    let loaded = loader.load(Some(JOB), now_ms());

    assert!(loaded.fan_shells.is_empty());
    assert_eq!(loaded.subagents[0].status, Status::Completed);
    assert_eq!(loaded.subagent_counts.running, 0);
}

#[test]
fn a_fan_that_ends_takes_its_shells_and_running_agent_away_at_the_next_load() {
    let world = World::new(Some(&fan()));
    let mut loader = Loader::new(DataRoot::fixture(&world.dir));
    assert_eq!(loader.load(Some(JOB), now_ms()).fan_shells.len(), 1);
    std::fs::write(
        world.dir.join(format!("claude/jobs/{SHORT}/state.json")),
        format!(
            r#"{{"state":"working","fan":[],"sessionId":"{JOB}","resumeSessionId":"{RESUME_ID}","name":"film research","cwd":"/work/film","updatedAt":"2026-11-17T03:26:00.983Z","linkScanPath":"claude/projects/-work-film/{RESUME_ID}.jsonl","template":"bg"}}"#
        ),
    )
    .expect("rewrite");
    let loaded = loader.load(Some(JOB), now_ms());
    assert!(loaded.fan_shells.is_empty());
    assert_eq!(loaded.subagents[0].status, Status::Completed);
}
