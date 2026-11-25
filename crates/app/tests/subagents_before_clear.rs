use std::fs::{File, FileTimes};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use notchling_app::chat::data::DataRoot;
use notchling_app::chat::loader::{Loaded, Loader};
use notchling_engine::bridge::net::files::now_ms;
use notchling_engine::transcript::subagents::Status;
use notchling_engine::transcript::tail::{PARSED_BYTES, RAW_BYTES};

const NAME: &str = "worker one";
const CURRENT: &str = "cur-1";
const REPORT: &str = "Report delivered to your caller";
const REPLY: &str = r#"{"type":"assistant","uuid":"a1","timestamp":"2026-11-12T10:00:00Z","message":{"id":"m1","role":"assistant","model":"claude-opus-5-5","content":[{"type":"text","text":"hi"}]}}"#;

static SERIAL: Mutex<()> = Mutex::new(());

struct World {
    dir: PathBuf,
    project: PathBuf,
    chain: Vec<String>,
}

fn pause() {
    std::thread::sleep(Duration::from_millis(40));
}

impl World {
    fn new(transcripts: usize) -> World {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "notchling-before-clear-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let project = dir.join("claude/projects/-work-app");
        std::fs::create_dir_all(&project).expect("folder");
        std::fs::create_dir_all(dir.join("claude/sessions")).expect("folder");
        let mut chain = Vec::new();
        for n in 0..transcripts {
            let id = if n + 1 == transcripts {
                CURRENT.to_string()
            } else {
                format!("old-{n}")
            };
            let head = if n == 0 {
                format!(r#"{{"type":"agent-name","agentName":"{NAME}"}}"#)
            } else {
                format!(
                    r#"{{"type":"system","agentName":"{NAME}","content":"SessionStart:clear"}}"#
                )
            };
            let reply = if n + 1 == transcripts {
                format!("{REPLY}\n")
            } else {
                String::new()
            };
            std::fs::write(project.join(format!("{id}.jsonl")), head + "\n" + &reply)
                .expect("transcript");
            chain.push(id);
            pause();
        }
        let pid = std::process::id();
        std::fs::write(
            dir.join(format!("claude/sessions/{pid}.json")),
            format!(
                r#"{{"sessionId":"{CURRENT}","pid":{pid},"cwd":"/work/app","status":"idle","name":"{NAME}","startedAt":1794160000000}}"#
            ),
        )
        .expect("session");
        World {
            dir,
            project,
            chain,
        }
    }

    fn folder(&self, transcript: &str) -> PathBuf {
        self.project.join(transcript).join("subagents")
    }

    fn add(&self, transcript: &str, id: &str, running: bool) -> PathBuf {
        let folder = self.folder(transcript);
        std::fs::create_dir_all(&folder).expect("subagents folder");
        let path = folder.join(format!("agent-{id}.jsonl"));
        let text = if running { "working" } else { REPORT };
        std::fs::write(
            &path,
            format!(
                r#"{{"isSidechain":true,"type":"assistant","message":{{"id":"m{id}","role":"assistant","model":"claude-sonnet-5-5","content":[{{"type":"text","text":"{text}"}}],"usage":{{"input_tokens":10,"cache_read_input_tokens":1000,"output_tokens":20}}}},"uuid":"u{id}","timestamp":"2026-11-08T08:00:05.000Z"}}"#
            ) + "\n",
        )
        .expect("agent");
        std::fs::write(
            folder.join(format!("agent-{id}.meta.json")),
            format!(r#"{{"description":"job {id}","agentType":"Explore","model":"sonnet"}}"#),
        )
        .expect("meta");
        if !running {
            age(&path);
        }
        pause();
        path
    }

    fn add_bare(&self, transcript: &str, id: &str) -> PathBuf {
        let folder = self.folder(transcript);
        std::fs::create_dir_all(&folder).expect("subagents folder");
        let path = folder.join(format!("agent-{id}.jsonl"));
        std::fs::write(&path, "working\n").expect("agent");
        pause();
        path
    }

    fn append(&self, transcript: &str, line: &str) {
        use std::io::Write;
        let mut file = File::options()
            .append(true)
            .open(self.project.join(format!("{transcript}.jsonl")))
            .expect("open");
        writeln!(file, "{line}").expect("append");
        pause();
    }

    fn pad(&self, transcript: &str, bytes: usize) {
        let line = format!(
            r#"{{"type":"assistant","message":{{"id":"p","role":"assistant","content":[{{"type":"text","text":"{}"}}]}}}}"#,
            "z".repeat(900)
        ) + "\n";
        let mut text = String::new();
        while text.len() < bytes {
            text.push_str(&line);
        }
        self.append(transcript, &text);
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn age(path: &Path) {
    let old = SystemTime::now() - Duration::from_secs(7200);
    File::options()
        .write(true)
        .open(path)
        .and_then(|f| f.set_times(FileTimes::new().set_modified(old)))
        .expect("age the file");
}

fn parsed() -> u64 {
    PARSED_BYTES.load(Ordering::Relaxed)
}

fn raw() -> u64 {
    RAW_BYTES.load(Ordering::Relaxed)
}

fn ids(loaded: &Loaded) -> Vec<String> {
    loaded
        .subagent_rows
        .iter()
        .map(|r| r.id.trim_start_matches("claude:").to_string())
        .collect()
}

fn load(loader: &mut Loader) -> (Loaded, u64) {
    let (parsed_before, raw_before) = (parsed(), raw());
    let loaded = loader.load(Some(CURRENT), now_ms());
    (loaded, parsed() - parsed_before + raw() - raw_before)
}

fn load_parsing(loader: &mut Loader) -> (Loaded, u64) {
    let before = parsed();
    let loaded = loader.load(Some(CURRENT), now_ms());
    (loaded, parsed() - before)
}

fn loader(world: &World) -> Loader {
    Loader::new(DataRoot::fixture(&world.dir))
}

#[test]
fn finished_subagents_of_the_transcript_a_clear_ended_are_listed() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let world = World::new(2);
    for id in ["o1", "o2", "o3", "o4"] {
        world.add(&world.chain[0], id, false);
    }
    world.add(CURRENT, "n1", false);
    let mut loader = loader(&world);

    let (first, _) = load(&mut loader);
    assert_eq!(first.subagents.len(), 5, "the list holds both folders");
    assert_eq!(first.subagent_counts.total, 5);
    assert_eq!(first.subagent_counts.hidden, 2);
    assert_eq!(ids(&first), ["n1", "o4", "o3"]);

    loader.more_subagents = 1;
    let (more, _) = load(&mut loader);
    assert_eq!(
        ids(&more).len(),
        5,
        "load more goes on into the earlier ones"
    );
    assert_eq!(more.subagent_counts.hidden, 0);
}

#[test]
fn an_earlier_subagent_opens_from_its_own_folder() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let world = World::new(2);
    let old = world.add(&world.chain[0], "o1", false);
    let mut loader = loader(&world);
    let (loaded, _) = load(&mut loader);
    let agent = loaded
        .subagents
        .iter()
        .find(|a| a.own_id() == "o1")
        .expect("listed");
    assert_eq!(agent.path, old.to_string_lossy());
    assert_eq!(agent.description, "job o1");
}

#[test]
fn an_id_in_both_folders_shows_once_from_the_newest_transcript() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let world = World::new(2);
    world.add(&world.chain[0], "o1", false);
    world.add(&world.chain[0], "dup", true);
    let new = world.add(CURRENT, "dup", true);
    let mut loader = loader(&world);

    let (loaded, _) = load(&mut loader);
    let dups: Vec<_> = loaded
        .subagents
        .iter()
        .filter(|a| a.own_id() == "dup")
        .collect();
    assert_eq!(dups.len(), 1);
    assert_eq!(dups[0].path, new.to_string_lossy());
    assert_eq!(loaded.subagent_counts.total, 2);
}

#[test]
fn running_ones_of_an_earlier_transcript_open_the_page_alone() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let world = World::new(2);
    world.add(&world.chain[0], "o1", false);
    world.add(&world.chain[0], "o2", true);
    world.add(CURRENT, "n1", false);
    let mut loader = loader(&world);
    let (first, _) = load(&mut loader);
    assert_eq!(ids(&first), ["o2"]);
    assert_eq!(first.subagent_counts.total, 3);
}

#[test]
fn the_walk_back_stops_after_a_few_transcripts() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let world = World::new(7);
    for (n, transcript) in world.chain.iter().enumerate() {
        world.add(transcript, &format!("t{n}"), false);
    }
    let mut loader = loader(&world);
    let (loaded, _) = load(&mut loader);
    let mut listed: Vec<String> = loaded
        .subagents
        .iter()
        .map(|a| a.own_id().to_string())
        .collect();
    listed.sort();
    assert_eq!(listed, ["t2", "t3", "t4", "t5", "t6"]);
}

#[test]
fn earlier_files_are_read_once_and_not_again_while_unchanged() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let world = World::new(3);
    for id in ["a1", "a2", "a3", "a4", "a5"] {
        world.add(&world.chain[0], id, false);
        world.add(&world.chain[1], &format!("b{id}"), false);
    }
    let mut loader = loader(&world);
    let (first, bytes) = load(&mut loader);
    assert_eq!(first.subagent_counts.total, 10);
    assert!(bytes > 0, "the first load reads");

    let (again, bytes) = load(&mut loader);
    assert_eq!(ids(&again), ids(&first));
    assert_eq!(bytes, 0, "nothing changed: nothing parsed, nothing scanned");
}

#[test]
fn an_earlier_transcript_is_scanned_for_its_endings_never_parsed_as_a_conversation() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let world = World::new(3);
    world.pad(&world.chain[0], 1_500_000);
    world.pad(&world.chain[1], 1_500_000);
    world.add(&world.chain[0], "a1", false);
    world.add(&world.chain[1], "b1", false);
    let mut loader = loader(&world);
    let (first, parsed_bytes) = load_parsing(&mut loader);
    assert_eq!(first.subagent_counts.total, 2);
    assert!(
        parsed_bytes < 100_000,
        "{parsed_bytes} bytes of the earlier transcripts went through the chat parser"
    );
}

#[test]
fn a_completion_delivered_after_the_clear_ends_a_subagent_of_the_earlier_transcript() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let world = World::new(2);
    world.add(&world.chain[0], "o1", true);
    world.add(&world.chain[0], "o2", true);
    world.append(
        CURRENT,
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-12T10:01:00Z","content":"<task-notification>\n<task-id>o1</task-id>\n<status>completed</status>\n</task-notification>"}"#,
    );
    let mut loader = loader(&world);
    let (loaded, _) = load(&mut loader);
    let status = |id: &str| {
        loaded
            .subagents
            .iter()
            .find(|a| a.own_id() == id)
            .map(|a| a.status)
    };
    assert_eq!(status("o1"), Some(Status::Completed));
    assert_eq!(status("o2"), Some(Status::Running));
    assert_eq!(ids(&loaded), ["o2"], "only the running one opens the page");
}

#[test]
fn a_subagent_running_at_the_clear_goes_on_in_the_new_folder_under_its_old_name() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let world = World::new(2);
    world.add(&world.chain[0], "dup", true);
    let continued = world.add_bare(CURRENT, "dup");
    let mut loader = loader(&world);
    let (loaded, _) = load(&mut loader);
    let rows: Vec<_> = loaded
        .subagents
        .iter()
        .filter(|a| a.own_id() == "dup")
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].path, continued.to_string_lossy());
    assert_eq!(rows[0].description, "job dup");
    assert_eq!(rows[0].agent_type, "Explore");
}
