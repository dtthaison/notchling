use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

use notchling_app::chat::data::DataRoot;
use notchling_app::chat::history;
use notchling_app::chat::loader::Loader;
use notchling_engine::bridge::net::files::now_ms;
use notchling_engine::settings::Settings;
use notchling_engine::transcript::tail::{chat_tail_bytes, set_chat_tail_bytes, PARSED_BYTES};
use serde_json::json;

const SESSION: &str = "term-1";
const LINES: usize = 600;
const TEXT: usize = 20_000;

static SERIAL: Mutex<()> = Mutex::new(());

struct World {
    dir: PathBuf,
}

impl World {
    fn new() -> World {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "notchling-chat-history-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let project = dir.join("claude/projects/-work-app");
        std::fs::create_dir_all(&project).expect("folder");
        std::fs::create_dir_all(dir.join("claude/sessions")).expect("folder");
        let filler = "x".repeat(TEXT);
        let mut text = String::new();
        for n in 0..LINES {
            let line = if n.is_multiple_of(2) {
                format!(
                    r#"{{"type":"user","message":{{"role":"user","content":"{n} {filler}"}},"uuid":"u{n}","timestamp":"2026-11-08T08:00:01.000Z"}}"#
                )
            } else {
                format!(
                    r#"{{"type":"assistant","message":{{"id":"m{n}","role":"assistant","model":"claude-sonnet-5-5","content":[{{"type":"text","text":"{n} {filler}"}}],"usage":{{"input_tokens":10,"output_tokens":20}}}},"uuid":"a{n}","timestamp":"2026-11-08T08:00:05.000Z"}}"#
                )
            };
            text.push_str(&line);
            text.push('\n');
        }
        std::fs::write(project.join(format!("{SESSION}.jsonl")), text).expect("transcript");
        let pid = std::process::id();
        std::fs::write(
            dir.join(format!("claude/sessions/{pid}.json")),
            format!(
                r#"{{"sessionId":"{SESSION}","pid":{pid},"cwd":"/work/app","status":"idle","name":"terminal one","startedAt":1794160000000}}"#
            ),
        )
        .expect("session");
        World { dir }
    }

    fn data_dir(&self) -> PathBuf {
        self.dir.join("notchling")
    }

    fn choose(&self, value: &str) {
        let file = self.data_dir().join("settings.json");
        let mut settings = Settings::open(&file).expect("open settings");
        settings.set("chatHistory", json!(value)).expect("set");
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn serial() -> MutexGuard<'static, ()> {
    let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    set_chat_tail_bytes(4_000_000);
    guard
}

fn parsed() -> u64 {
    PARSED_BYTES.load(Ordering::Relaxed)
}

fn load(loader: &mut Loader) -> (usize, u64) {
    let before = parsed();
    let loaded = loader.load(Some(SESSION), now_ms());
    (loaded.conversation.items.len(), parsed() - before)
}

#[test]
fn loads_never_read_the_setting_and_the_start_of_the_app_does() {
    let _serial = serial();
    let world = World::new();
    let mut loader = Loader::new(DataRoot::fixture(&world.dir));
    world.choose("16mb");

    let (default, first) = load(&mut loader);
    assert!(first > 0);
    assert_eq!(
        chat_tail_bytes(),
        4_000_000,
        "a load does not open the store"
    );
    assert_eq!(load(&mut loader), (default, 0), "unchanged: nothing read");

    history::apply(&world.data_dir());
    assert_eq!(chat_tail_bytes(), 16_000_000, "the app start reads it once");
    let (more, cost) = load(&mut loader);
    assert!(cost > 0, "the cursor was built with the old budget");
    assert!(more > default, "a larger budget loads more of a long file");
    assert_eq!(load(&mut loader), (more, 0));
}

#[test]
fn a_change_in_the_sheet_applies_at_once_and_one_load_builds_the_cursor_again() {
    let _serial = serial();
    let world = World::new();
    let mut loader = Loader::new(DataRoot::fixture(&world.dir));
    let (default, _) = load(&mut loader);

    let mut kept = Vec::new();
    let mut reloads = 0;
    for (choice, bytes) in [
        ("2mb", 2_000_000),
        ("4mb", 4_000_000),
        ("8mb", 8_000_000),
        ("16mb", 16_000_000),
    ] {
        if history::on_change("chatHistory", &json!(choice)) {
            reloads += 1;
        }
        assert_eq!(chat_tail_bytes(), bytes, "{choice}: set before any load");
        let (items, cost) = load(&mut loader);
        assert!(cost > 0, "{choice}: the one reload parses again");
        assert_eq!(load(&mut loader), (items, 0), "{choice}: and only one");
        kept.push(items);
    }
    assert_eq!(reloads, 4, "each change asks for one reload");
    assert!(
        kept.windows(2).all(|w| w[0] < w[1]),
        "a larger budget loads more of a long file: {kept:?}"
    );
    assert_eq!(kept[1], default, "four megabytes is what it was");

    assert!(
        !history::on_change("chatHistory", &json!("16mb")),
        "the same again"
    );
    assert!(
        !history::on_change("chatHistory", &json!("1gb")),
        "not a choice"
    );
    assert!(
        !history::on_change("chatUsedSessions", &json!("2mb")),
        "another key"
    );
    assert_eq!(chat_tail_bytes(), 16_000_000);
    assert_eq!(
        load(&mut loader),
        (kept[3], 0),
        "none of them asked for a reload"
    );
}

#[test]
fn a_store_that_cannot_be_read_keeps_the_budget_it_had() {
    let _serial = serial();
    let world = World::new();
    assert!(history::on_change("chatHistory", &json!("8mb")));
    std::fs::create_dir_all(world.data_dir()).expect("folder");
    std::fs::write(world.data_dir().join("notchling.db"), "not a database").expect("write");
    assert_eq!(history::read(&world.data_dir()), None);
    history::apply(&world.data_dir());
    assert_eq!(
        chat_tail_bytes(),
        8_000_000,
        "an error is not four megabytes"
    );
}
