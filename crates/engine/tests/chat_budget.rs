use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use notchling_engine::transcript::subagents;
use notchling_engine::transcript::tail::{
    chat_tail_bytes, set_chat_tail_bytes, CHAT_TAIL_BYTES, PARSED_BYTES,
};
use notchling_engine::transcript::{codex, conversation};

const LINES: usize = 1000;
const TEXT: usize = 1000;

struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "chat-budget-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("folder");
        Dir(dir)
    }

    fn file(&self, name: &str, line: impl Fn(usize) -> String) -> String {
        let path: &Path = &self.0.join(name);
        let mut text = (0..LINES).map(line).collect::<Vec<_>>().join("\n");
        text.push('\n');
        std::fs::write(path, text).expect("write");
        path.to_string_lossy().into_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn claude_line(n: usize, sidechain: bool) -> String {
    let text = format!("{n} {}", "x".repeat(TEXT));
    if n.is_multiple_of(2) {
        format!(
            r#"{{"isSidechain":{sidechain},"type":"user","message":{{"role":"user","content":"{text}"}},"uuid":"u{n}","timestamp":"2026-11-08T08:00:01.000Z"}}"#
        )
    } else {
        format!(
            r#"{{"isSidechain":{sidechain},"type":"assistant","message":{{"id":"m{n}","role":"assistant","model":"claude-sonnet-5-5","content":[{{"type":"text","text":"{text}"}}],"usage":{{"input_tokens":10,"output_tokens":20}}}},"uuid":"a{n}","timestamp":"2026-11-08T08:00:05.000Z"}}"#
        )
    }
}

fn codex_line(n: usize) -> String {
    format!(
        r#"{{"timestamp":"2026-11-12T10:00:01.000Z","type":"response_item","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"step {n} {}"}}]}}}}"#,
        "q".repeat(TEXT)
    )
}

fn parsed() -> u64 {
    PARSED_BYTES.load(Ordering::Relaxed)
}

fn walk(name: &str, read: &dyn Fn() -> usize) {
    let mut seen = Vec::new();
    for budget in [100_000u64, 200_000, 400_000, 4_000_000] {
        set_chat_tail_bytes(budget);
        let before = parsed();
        let items = read();
        let cost = parsed() - before;
        assert!(
            cost > 0,
            "{name}: a changed budget builds the cursor again at {budget}"
        );
        let again = parsed();
        assert_eq!(read(), items);
        assert_eq!(
            parsed() - again,
            0,
            "{name}: the same budget reuses the cursor"
        );
        seen.push((budget, items, cost));
    }
    let counts: Vec<usize> = seen.iter().map(|s| s.1).collect();
    assert!(
        counts.windows(2).all(|w| w[0] < w[1]),
        "{name}: a larger budget keeps more: {counts:?}"
    );
    for (budget, _, cost) in &seen[..3] {
        assert!(
            *cost <= budget + 2 * (TEXT as u64 + 500),
            "{name}: {cost} bytes read at {budget}"
        );
    }
    set_chat_tail_bytes(100_000_000);
    assert_eq!(
        counts[3],
        read(),
        "{name}: a budget past the file keeps all of it"
    );

    set_chat_tail_bytes(100_000);
    let small = read();
    set_chat_tail_bytes(400_000);
    let large = read();
    set_chat_tail_bytes(100_000);
    assert_eq!(
        read(),
        small,
        "{name}: going back to a smaller budget drops the older turns again"
    );
    assert!(large > small);
}

#[test]
fn the_chat_budget_decides_how_much_of_a_file_each_chat_read_keeps() {
    assert_eq!(
        chat_tail_bytes(),
        CHAT_TAIL_BYTES,
        "four megabytes at start"
    );
    let dir = Dir::new();
    let session = dir.file("session.jsonl", |n| claude_line(n, false));
    let sub = dir.file("agent-a1.jsonl", |n| claude_line(n, true));
    let rollout = dir.file("rollout.jsonl", codex_line);

    walk("session", &|| {
        conversation::read_shared(&session).items.len()
    });
    walk("subagent", &|| subagents::read_shared(&sub).items.len());
    walk("codex", &|| codex::read_shared(&rollout).items.len());

    set_chat_tail_bytes(CHAT_TAIL_BYTES);
}
