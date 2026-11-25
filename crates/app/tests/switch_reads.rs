use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use notchling_app::chat::data::{DataRoot, Loader};
use notchling_engine::bridge::net::files::now_ms;
use notchling_engine::delegate::options::{Options, Tool as DelegateTool};
use notchling_engine::delegate::record::{self, Delegation};
use notchling_engine::tools::codex::children::{ChildSource, DayFolders, Head};
use notchling_engine::transcript::tail::RAW_BYTES;

const SESSION: &str = "term-1";
const JOBS: usize = 40;

static SERIAL: Mutex<()> = Mutex::new(());

struct World {
    dir: PathBuf,
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn thread(n: usize) -> String {
    format!("01a1d5da-0000-7000-8000-{n:012}")
}

fn world() -> World {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "notchling-switch-reads-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let project = dir.join("claude/projects/-work-app");
    std::fs::create_dir_all(&project).expect("folder");
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
    let day = dir.join("codex/sessions/2026/11/12");
    std::fs::create_dir_all(&day).expect("folder");
    let records = dir.join("notchling").join(record::FOLDER);
    for n in 0..JOBS {
        let id = thread(n);
        std::fs::write(
            day.join(format!("rollout-2026-11-12T09-00-00-{id}.jsonl")),
            format!(
                "{{\"timestamp\":\"2026-11-12T09:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"cwd\":\"/work/app\"}}}}\n"
            ),
        )
        .expect("rollout");
        let options = Options {
            tool: DelegateTool::Codex,
            prompt: format!("Review part {n}"),
            model: Some("gpt-5.5".into()),
            effort: None,
            folder: "/work/app".into(),
            out: None,
            write: false,
            session: None,
            timeout: std::time::Duration::from_secs(30),
        };
        let started = 1_794_160_100.0 + n as f64;
        let mut job = Delegation::new(SESSION, "/work/app", &options, started, 1);
        job.session = Some(id);
        job.ended = Some(1_794_160_200.0);
        record::write(&records, &job).expect("record");
    }
    World { dir }
}

fn raw() -> u64 {
    RAW_BYTES.load(Ordering::Relaxed)
}

struct Counting(AtomicUsize);

impl ChildSource for Counting {
    fn children(&self, parent_thread: &str, parent_path: &std::path::Path) -> Vec<Head> {
        self.0.fetch_add(1, Ordering::SeqCst);
        DayFolders.children(parent_thread, parent_path)
    }
}

#[test]
fn a_second_load_reads_no_codex_rollout_head_again() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let mut loader = Loader::new(DataRoot::fixture(&world.dir));
    let before = raw();
    let first = loader.load(Some(SESSION), now_ms());
    let first_bytes = raw() - before;
    assert_eq!(first.subagents.len(), JOBS, "{:?}", first.subagents);
    assert!(first_bytes > 0, "the heads are read once");

    let before = raw();
    let again = loader.load(Some(SESSION), now_ms());
    assert_eq!(again.subagents.len(), JOBS);
    assert_eq!(raw() - before, 0, "nothing changed: no head read again");
}

#[test]
fn a_switch_shows_the_conversation_before_any_subagent_is_listed() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let world = world();
    let source = Arc::new(Counting(AtomicUsize::new(0)));
    let mut loader =
        Loader::with_children(DataRoot::fixture(&world.dir), source.clone()).quick_on_switch();

    let first = loader.load(Some(SESSION), now_ms());
    assert!(first.partial);
    assert_eq!(first.selected.as_deref(), Some(SESSION));
    assert!(!first.conversation.items.is_empty());
    assert!(first.subagents.is_empty() && first.subagent_rows.is_empty());
    assert_eq!(
        source.0.load(Ordering::SeqCst),
        0,
        "no subagent was asked for"
    );

    let full = loader.load(Some(SESSION), now_ms());
    assert!(!full.partial);
    assert_eq!(full.subagents.len(), JOBS);
    assert_eq!(source.0.load(Ordering::SeqCst), JOBS);
}
