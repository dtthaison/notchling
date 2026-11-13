use std::fs::{File, FileTimes};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use notchling_app::chat::data::DataRoot;
use notchling_app::chat::loader::{Loaded, Loader};
use notchling_engine::bridge::net::files::now_ms;
use notchling_engine::transcript::glance::FIRST;
use notchling_engine::transcript::tail::PARSED_BYTES;

const FILE_BYTES: usize = 1_000_000;
const SESSION: &str = "term-1";

static SERIAL: Mutex<()> = Mutex::new(());

struct World {
    dir: PathBuf,
    next: usize,
}

impl World {
    fn new() -> World {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "notchling-subagent-reads-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let project = dir.join("claude/projects/-work-app");
        std::fs::create_dir_all(project.join(format!("{SESSION}/subagents"))).expect("folder");
        std::fs::create_dir_all(dir.join("claude/sessions")).expect("folder");
        std::fs::write(
            project.join(format!("{SESSION}.jsonl")),
            "{\"type\":\"user\",\"timestamp\":\"2026-11-08T08:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n",
        )
        .expect("transcript");
        let pid = std::process::id();
        std::fs::write(
            dir.join(format!("claude/sessions/{pid}.json")),
            format!(
                r#"{{"sessionId":"{SESSION}","pid":{pid},"cwd":"/work/app","status":"idle","name":"terminal one","startedAt":1794160000000}}"#
            ),
        )
        .expect("session");
        World { dir, next: 0 }
    }

    fn folder(&self) -> PathBuf {
        self.dir
            .join(format!("claude/projects/-work-app/{SESSION}/subagents"))
    }

    fn add(&mut self, running: bool) -> String {
        let n = self.next;
        self.next += 1;
        let id = format!("b{n:03}");
        let path = self.folder().join(format!("agent-{id}.jsonl"));
        write_agent(&path, FILE_BYTES);
        std::fs::write(
            self.folder().join(format!("agent-{id}.meta.json")),
            format!(r#"{{"description":"job {n}","agentType":"Explore","model":"sonnet"}}"#),
        )
        .expect("meta");
        if !running {
            let old = SystemTime::now() - Duration::from_secs(7200);
            File::options()
                .write(true)
                .open(&path)
                .and_then(|f| f.set_times(FileTimes::new().set_modified(old)))
                .expect("age the file");
        }
        id
    }

    fn touch(&self, id: &str) {
        let path = self.folder().join(format!("agent-{id}.jsonl"));
        let mut file = File::options().append(true).open(path).expect("open");
        writeln!(file, "{}", line(99_999, "more")).expect("append");
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn line(n: usize, text: &str) -> String {
    format!(
        r#"{{"isSidechain":true,"type":"assistant","message":{{"id":"m{n}","role":"assistant","model":"claude-sonnet-5-5","content":[{{"type":"text","text":"{text}"}}],"usage":{{"input_tokens":10,"cache_read_input_tokens":{},"output_tokens":20}}}},"uuid":"a{n}","timestamp":"2026-11-08T08:00:05.000Z"}}"#,
        1000 + n
    )
}

fn write_agent(path: &Path, bytes: usize) {
    let mut file = File::create(path).expect("create");
    let filler = "x".repeat(700);
    let mut written = 0;
    let mut n = 0;
    while written < bytes {
        n += 1;
        let text = line(n, &filler);
        writeln!(file, "{text}").expect("write");
        written += text.len() + 1;
    }
}

fn parsed() -> u64 {
    PARSED_BYTES.load(Ordering::Relaxed)
}

fn ids(loaded: &Loaded) -> Vec<String> {
    loaded.subagent_rows.iter().map(|r| r.id.clone()).collect()
}

fn want(ids: &[&String]) -> Vec<String> {
    ids.iter().map(|id| (*id).clone()).collect()
}

fn load(loader: &mut Loader) -> (Loaded, u64) {
    let before = parsed();
    let loaded = loader.load(Some(SESSION), now_ms());
    (loaded, parsed() - before)
}

fn loader(world: &World) -> Loader {
    Loader::new(DataRoot::fixture(&world.dir))
}

const SMALL: u64 = 4096;

#[test]
fn with_forty_and_two_running_the_first_load_shows_and_reads_exactly_two() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut world = World::new();
    let all: Vec<String> = (0..40).map(|n| world.add(n == 10 || n == 25)).collect();
    let mut loader = loader(&world);

    let (first, bytes) = load(&mut loader);
    assert_eq!(ids(&first), [all[25].clone(), all[10].clone()]);
    assert_eq!(first.subagents.len(), 40, "the list itself is whole");
    let counts = first.subagent_counts;
    assert_eq!((counts.total, counts.running, counts.hidden), (40, 2, 38));
    assert!(bytes > 0, "the shown rows were read");
    assert!(
        bytes <= 2 * 2 * FIRST + SMALL,
        "two files, each read for its number and its doing line, from a small tail: {bytes}"
    );
    let detail = &first.subagent_rows[0].detail;
    assert!(
        detail.starts_with("Running · sonnet · Explore · ") && detail.ends_with('k'),
        "{detail}"
    );
    assert_eq!(first.subagent_rows[0].doing.as_deref(), Some("Writing…"));

    let (_, again) = load(&mut loader);
    assert_eq!(again, 0, "nothing changed: nothing read");

    world.touch(&all[10]);
    let (touched, bytes) = load(&mut loader);
    assert_eq!(ids(&touched).len(), 2);
    assert!(
        bytes > 0 && bytes <= 2 * FIRST + SMALL,
        "only the file that grew: {bytes}"
    );
}

#[test]
fn with_none_running_the_first_load_shows_and_reads_three_and_each_more_reads_ten() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut world = World::new();
    let all: Vec<String> = (0..40).map(|_| world.add(false)).collect();
    let mut loader = loader(&world);

    let (first, bytes) = load(&mut loader);
    assert_eq!(ids(&first), want(&[&all[39], &all[38], &all[37]]));
    assert!(bytes > 0 && bytes <= 3 * FIRST + SMALL, "{bytes}");

    let (again, bytes) = load(&mut loader);
    assert_eq!((ids(&again).len(), bytes), (3, 0));

    loader.more_subagents = 1;
    let (more, bytes) = load(&mut loader);
    assert_eq!(ids(&more).len(), 13);
    assert_eq!(more.subagent_counts.hidden, 27);
    assert!(
        bytes > 0 && bytes <= 10 * FIRST + SMALL,
        "ten new files: {bytes}"
    );

    let (_, bytes) = load(&mut loader);
    assert_eq!(bytes, 0, "an unchanged file is never read again");

    for _ in 0..3 {
        loader.more_subagents = 1;
        load(&mut loader);
    }
    let (last, _) = load(&mut loader);
    assert_eq!(ids(&last).len(), 40);
    assert_eq!(last.subagent_counts.hidden, 0);
}

#[test]
fn a_new_subagent_goes_to_the_top_and_the_three_done_ones_stay() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut world = World::new();
    let all: Vec<String> = (0..12).map(|_| world.add(false)).collect();
    let mut loader = loader(&world);
    let (first, _) = load(&mut loader);
    assert_eq!(ids(&first), want(&[&all[11], &all[10], &all[9]]));

    let fresh = world.add(true);
    let (second, bytes) = load(&mut loader);
    assert_eq!(ids(&second), want(&[&fresh, &all[11], &all[10], &all[9]]));
    assert_eq!(second.subagent_counts.total, 13);
    assert!(
        bytes <= 2 * FIRST + SMALL,
        "only the new one is read: {bytes}"
    );

    loader.more_subagents = 1;
    let (more, _) = load(&mut loader);
    assert_eq!(
        ids(&more).len(),
        13,
        "load more goes on below the ones shown"
    );
    assert_eq!(more.subagent_counts.hidden, 0);
}

#[test]
fn a_running_one_that_finishes_while_shown_stays_shown() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut world = World::new();
    let all: Vec<String> = (0..6).map(|n| world.add(n == 2)).collect();
    let mut loader = loader(&world);
    let (first, _) = load(&mut loader);
    assert_eq!(ids(&first), want(&[&all[2]]));

    let path = world.folder().join(format!("agent-{}.jsonl", all[2]));
    let old = SystemTime::now() - Duration::from_secs(7200);
    File::options()
        .write(true)
        .open(path)
        .and_then(|f| f.set_times(FileTimes::new().set_modified(old)))
        .expect("age the file");
    let (second, _) = load(&mut loader);
    assert_eq!(ids(&second), want(&[&all[2]]));
    assert_eq!(second.subagent_counts.running, 0);
}

#[test]
fn a_running_subagent_is_read_once_for_its_number_and_its_doing_line() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut world = World::new();
    let id = world.add(true);
    let mut loader = loader(&world);

    let (first, bytes) = load(&mut loader);
    assert_eq!(ids(&first), want(&[&id]));
    let row = &first.subagent_rows[0];
    assert!(row.detail.ends_with('k'), "its number: {}", row.detail);
    assert_eq!(row.doing.as_deref(), Some("Writing…"));
    assert!(
        bytes > 0 && bytes <= FIRST + SMALL,
        "one small tail serves both, not two: {bytes}"
    );
}
