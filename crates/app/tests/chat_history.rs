use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use notchling_app::chat::data::DataRoot;
use notchling_app::chat::loader::Loader;
use notchling_engine::bridge::net::files::now_ms;
use notchling_engine::settings::Settings;
use notchling_engine::transcript::tail::{chat_tail_bytes, PARSED_BYTES};
use serde_json::json;

const SESSION: &str = "term-1";
const LINES: usize = 600;
const TEXT: usize = 20_000;

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

    fn choose(&self, value: &str) {
        let file = self.dir.join("notchling/settings.json");
        let mut settings = Settings::open(&file).expect("open settings");
        settings.set("chatHistory", json!(value)).expect("set");
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
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
fn the_setting_is_read_at_each_load_and_a_change_builds_the_cursor_again() {
    let world = World::new();
    let mut loader = Loader::new(DataRoot::fixture(&world.dir));

    let (default, first) = load(&mut loader);
    assert_eq!(chat_tail_bytes(), 4_000_000, "four megabytes until chosen");
    assert!(first > 0);
    assert_eq!(load(&mut loader), (default, 0), "unchanged: nothing read");

    let mut kept = Vec::new();
    for (choice, bytes) in [
        ("2mb", 2_000_000),
        ("4mb", 4_000_000),
        ("8mb", 8_000_000),
        ("16mb", 16_000_000),
    ] {
        world.choose(choice);
        let (items, cost) = load(&mut loader);
        assert_eq!(chat_tail_bytes(), bytes);
        assert!(
            cost > 0,
            "{choice}: the cursor was built with the old budget"
        );
        assert_eq!(load(&mut loader), (items, 0), "{choice}: now unchanged");
        kept.push(items);
    }
    assert!(
        kept.windows(2).all(|w| w[0] < w[1]),
        "a larger budget loads more of a long file: {kept:?}"
    );
    assert_eq!(kept[1], default, "four megabytes is what it was");
}
