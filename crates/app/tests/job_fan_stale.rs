use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use notchling_app::chat::data::DataRoot;
use notchling_app::chat::inspector::inspector_with_fan;
use notchling_app::chat::loader::Loader;
use notchling_app::chat::shell_list::{probe_paths_with_fan, ShellPage};
use notchling_engine::bridge::net::files::now_ms;

const MAIN: &str = include_str!("../../engine/tests/fixtures/background-job/main.jsonl");
const JOB: &str = "job-film-1";
const SHORT: &str = "9f3883da";
const RESUME_ID: &str = "5e5510aa-0000-4000-8000-000000000001";
const LONG_AGO: i64 = 1_794_000_000_000;

struct World {
    dir: PathBuf,
}

impl World {
    fn new(fan: &str) -> World {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "notchling-job-fan-stale-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let project = dir.join("claude/projects/-work-film");
        std::fs::create_dir_all(&project).expect("folder");
        let main: String = MAIN.lines().take(4).map(|l| format!("{l}\n")).collect();
        std::fs::write(project.join(format!("{RESUME_ID}.jsonl")), main).expect("transcript");
        let job = dir.join(format!("claude/jobs/{SHORT}"));
        std::fs::create_dir_all(&job).expect("job folder");
        std::fs::write(
            job.join("state.json"),
            format!(
                r#"{{"state":"working","tempo":"active","fan":{fan},"sessionId":"{JOB}","resumeSessionId":"{RESUME_ID}","name":"film research","cwd":"/work/film","updatedAt":"2026-11-17T03:25:00.983Z","linkScanPath":"claude/projects/-work-film/{RESUME_ID}.jsonl","template":"bg"}}"#
            ),
        )
        .expect("state");
        World { dir }
    }

    fn output_of(&self, id: &str) -> PathBuf {
        let tasks = self
            .dir
            .join("tmp/claude-501/-work-film")
            .join(RESUME_ID)
            .join("tasks");
        std::fs::create_dir_all(&tasks).expect("tasks folder");
        let file = tasks.join(format!("{id}.output"));
        std::fs::write(&file, "").expect("output");
        file
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn shell(id: &str, label: &str, started_ms: i64) -> String {
    format!(r#"{{"id":"{id}","kind":"shell","label":"{label}","startedAt":{started_ms}}}"#)
}

#[test]
fn a_fan_shell_whose_output_file_is_gone_is_not_listed_as_running() {
    let world = World::new(&format!(
        "[{},{}]",
        shell("bsub000001", "until grep -q done /tmp/a.log", LONG_AGO),
        shell("bsub000002", "sleep 900", LONG_AGO)
    ));
    world.output_of("bsub000002");
    let mut loader = Loader::new(DataRoot::fixture(&world.dir));
    let loaded = loader.load(Some(JOB), now_ms());

    let ids: Vec<&str> = loaded.fan_shells.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["bsub000002"], "{:?}", loaded.fan_shells);
}

#[test]
fn a_fan_shell_with_an_output_file_carries_its_path_for_the_probe() {
    let world = World::new(&format!("[{}]", shell("bsub000001", "sleep 900", LONG_AGO)));
    let file = world.output_of("bsub000001");
    let mut loader = Loader::new(DataRoot::fixture(&world.dir));
    let loaded = loader.load(Some(JOB), now_ms());

    assert_eq!(loaded.fan_shells.len(), 1);
    assert_eq!(loaded.fan_shells[0].output_path, file.to_string_lossy());
}

#[test]
fn a_fan_shell_just_started_is_listed_before_its_output_file_exists() {
    let world = World::new(&format!(
        "[{}]",
        shell("bsub000001", "sleep 900", now_ms() - 5_000)
    ));
    let mut loader = Loader::new(DataRoot::fixture(&world.dir));
    let loaded = loader.load(Some(JOB), now_ms());

    assert_eq!(loaded.fan_shells.len(), 1);
    assert!(loaded.fan_shells[0].output_path.is_empty());
}

#[test]
fn a_fan_shell_no_process_writes_to_is_an_ended_row_and_not_counted_as_running() {
    let world = World::new(&format!("[{}]", shell("bsub000001", "sleep 900", LONG_AGO)));
    let file = world.output_of("bsub000001");
    let path = file.to_string_lossy().into_owned();
    let mut loader = Loader::new(DataRoot::fixture(&world.dir));
    let loaded = loader.load(Some(JOB), now_ms());

    let mut page = ShellPage::default();
    let listing = page.show_with_fan(
        Some(JOB),
        &loaded.conversation,
        &loaded.fan_shells,
        0,
        &|p| Some(p != path),
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
    assert!(!i.shells[0].running);
    assert_eq!(i.shells[0].status_word, "Ended");
    assert_eq!(listing.counts.running, 0);
    assert_eq!(i.shells_title(), "Shells · 1");
}

#[test]
fn a_fan_shell_a_process_still_writes_to_stays_a_running_row() {
    let world = World::new(&format!("[{}]", shell("bsub000001", "sleep 900", LONG_AGO)));
    let file = world.output_of("bsub000001");
    let path = file.to_string_lossy().into_owned();
    let mut loader = Loader::new(DataRoot::fixture(&world.dir));
    let loaded = loader.load(Some(JOB), now_ms());

    let mut page = ShellPage::default();
    let listing = page.show_with_fan(
        Some(JOB),
        &loaded.conversation,
        &loaded.fan_shells,
        0,
        &|p| Some(p == path),
    );
    let i = inspector_with_fan(
        &loaded.conversation,
        &loaded.fan_shells,
        &loaded.subagent_rows,
        loaded.subagent_counts,
        Some(&listing),
        &|_| vec![4242],
    );
    assert_eq!(i.shells.len(), 1);
    assert!(i.shells[0].running);
    assert_eq!(listing.counts.running, 1);
    assert_eq!(i.shells_title(), "Shells · 1 running");
}

#[test]
fn a_fan_shell_is_listed_while_no_claude_folder_says_where_shells_write() {
    let world = World::new(&format!("[{}]", shell("bsub000001", "sleep 900", LONG_AGO)));
    std::fs::create_dir_all(world.dir.join("tmp/other")).expect("tmp");
    let mut loader = Loader::new(DataRoot::fixture(&world.dir));
    let loaded = loader.load(Some(JOB), now_ms());

    assert_eq!(loaded.fan_shells.len(), 1);
    assert!(loaded.fan_shells[0].output_path.is_empty());
}

#[test]
fn the_probe_asks_about_the_output_file_of_a_fan_shell_and_stops_once_it_is_idle() {
    let world = World::new(&format!("[{}]", shell("bsub000001", "sleep 900", LONG_AGO)));
    let file = world.output_of("bsub000001");
    let path = file.to_string_lossy().into_owned();
    let mut loader = Loader::new(DataRoot::fixture(&world.dir));
    let loaded = loader.load(Some(JOB), now_ms());
    let (c, fan) = (&loaded.conversation, &loaded.fan_shells);

    let mut page = ShellPage::default();
    let unknown = |_: &str| None;
    let listing = page.show_with_fan(Some(JOB), c, fan, 0, &unknown);
    assert_eq!(
        probe_paths_with_fan(c, fan, &listing, &unknown),
        std::slice::from_ref(&path)
    );

    let idle = |_: &str| Some(false);
    let listing = page.show_with_fan(Some(JOB), c, fan, 0, &idle);
    assert!(probe_paths_with_fan(c, fan, &listing, &idle).is_empty());
}

#[test]
fn a_fan_shell_the_conversation_saw_end_is_an_ended_row_whatever_the_fan_says() {
    let world = World::new(&format!(
        "[{}]",
        shell("bsub000001", "sleep 900", now_ms() - 5_000)
    ));
    let mut loader = Loader::new(DataRoot::fixture(&world.dir));
    let loaded = loader.load(Some(JOB), now_ms());
    let mut conversation = (*loaded.conversation).clone();
    conversation
        .finished_agents
        .insert("bsub000001".into(), "completed".into());

    let mut page = ShellPage::default();
    let listing = page.show_with_fan(Some(JOB), &conversation, &loaded.fan_shells, 0, &|_| None);
    let i = inspector_with_fan(
        &conversation,
        &loaded.fan_shells,
        &loaded.subagent_rows,
        loaded.subagent_counts,
        Some(&listing),
        &|_| Vec::new(),
    );
    assert_eq!(i.shells.len(), 1);
    assert_eq!(i.shells[0].status_word, "Completed");
    assert_eq!(listing.counts.running, 0);
}
