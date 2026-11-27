use super::chunk::*;
use super::{SessionRole, TranscriptLine};

fn user(text: &str) -> TranscriptLine {
    TranscriptLine::Message {
        role: SessionRole::User,
        text: text.to_string(),
        cwd: None,
    }
}

fn assistant(text: &str) -> TranscriptLine {
    TranscriptLine::Message {
        role: SessionRole::Assistant,
        text: text.to_string(),
        cwd: None,
    }
}

fn sentences(n: usize) -> String {
    (0..n)
        .map(|i| format!("Sentence number {i:04} says something useful. "))
        .collect()
}

fn turn(prompt: &str, reply: &str) -> Turn {
    Turn {
        prompt: prompt.to_string(),
        reply: reply.to_string(),
        closed: true,
    }
}

#[test]
fn turns_pair_prompts_with_replies() {
    let t = turns(
        vec![
            assistant("stray"),
            TranscriptLine::AiTitle("t".to_string()),
            user("one"),
            assistant("a1"),
            assistant("a2"),
            user("two"),
        ],
        true,
    );
    assert_eq!(t.len(), 2);
    assert_eq!(t[0].prompt, "one");
    assert_eq!(t[0].reply, "a1\n\na2");
    assert!(t[0].closed);
    assert_eq!(t[1].reply, "");
    assert!(t[1].closed);
}

#[test]
fn the_open_turn_waits() {
    let lines = vec![
        user("first"),
        assistant("r1"),
        user("second"),
        assistant("r2"),
    ];
    let open = turns(lines.clone(), false);
    assert!(open[0].closed);
    assert!(!open[1].closed);
    let chunks = session_chunks("T", "/a/b", &open);
    assert!(chunks.iter().any(|c| c.text.contains("r1")));
    assert!(!chunks.iter().any(|c| c.text.contains("r2")));
    let quiet = turns(lines, true);
    assert!(quiet[1].closed);
    assert!(session_chunks("T", "/a/b", &quiet)
        .iter()
        .any(|c| c.text.contains("r2")));
}

#[test]
fn the_head_chunk_names_title_folder_and_first_prompt() {
    let t = vec![turn("how do I log in", "r")];
    let chunks = session_chunks("Login work", "/Users/x/proj/", &t);
    assert_eq!(chunks[0].kind, ChunkKind::Head);
    assert_eq!(chunks[0].text, "Login work\nproj\nhow do I log in");
    let long = vec![turn(&"x".repeat(2000), "")];
    let head = &session_chunks("T", "f", &long)[0];
    assert_eq!(head.text.chars().count(), 600);
    assert!(session_chunks("", "", &[]).is_empty());
}

#[test]
fn windows_respect_size_and_overlap() {
    let t = vec![turn("prompt here", &sentences(60))];
    let chunks = session_chunks("", "", &t);
    let bodies: Vec<&ChunkText> = chunks
        .iter()
        .filter(|c| c.kind == ChunkKind::Turn)
        .collect();
    assert!(bodies.len() >= 2);
    for c in &bodies {
        assert!(c.text.chars().count() <= 1200, "{}", c.text.chars().count());
        assert!(c.text.starts_with("prompt here\n\n"));
    }
    for pair in bodies.windows(2) {
        let prev = pair[0].text.trim_end();
        let tail: String = prev
            .chars()
            .rev()
            .take(100)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert!(pair[1].text.contains(&tail), "no overlap");
    }
    let joined: String = bodies.iter().map(|c| c.text.as_str()).collect();
    assert!(joined.contains("Sentence number 0000"));
    assert!(joined.contains("Sentence number 0059"));
}

#[test]
fn the_prompt_prefix_is_cut_to_two_hundred() {
    let t = vec![turn(&"p".repeat(500), "reply")];
    let c = &session_chunks("", "", &t)[1];
    assert_eq!(c.text, format!("{}\n\nreply", "p".repeat(200)));
}

#[test]
fn code_blocks_keep_twenty_lines() {
    let code: String = (1..=50).map(|i| format!("code line {i}\n")).collect();
    let reply = format!("Before.\n\n```rust\n{code}```\n\nAfter.");
    let t = vec![turn("q", &reply)];
    let chunks = session_chunks("", "", &t);
    let text = &chunks.last().unwrap().text;
    assert!(text.contains("code line 20\n"));
    assert!(!text.contains("code line 21"));
    assert!(text.contains("After."));
    assert!(text.contains("Before."));
}

#[test]
fn caps_six_windows_and_two_hundred_chunks() {
    let t = vec![turn("q", &sentences(2000))];
    let chunks = session_chunks("T", "f", &t);
    assert_eq!(
        chunks.iter().filter(|c| c.kind == ChunkKind::Turn).count(),
        6
    );
    let many: Vec<Turn> = (0..300)
        .map(|i| turn(&format!("prompt {i}"), "reply"))
        .collect();
    let chunks = session_chunks("T", "f", &many);
    assert_eq!(chunks.len(), 200);
    assert_eq!(chunks[0].kind, ChunkKind::Head);
    assert!(chunks[1].text.starts_with("prompt 299"));
    assert!(chunks.iter().all(|c| !c.text.starts_with("prompt 0\n")));
}

#[test]
fn a_grown_transcript_keeps_old_hashes() {
    let mut lines = vec![
        user("one"),
        assistant(&sentences(30)),
        user("two"),
        assistant("r2"),
    ];
    let before: Vec<[u8; 16]> = session_chunks("T", "/p", &turns(lines.clone(), true))
        .iter()
        .map(|c| hash(&c.text))
        .collect();
    lines.push(user("three"));
    lines.push(assistant("r3"));
    let after: Vec<[u8; 16]> = session_chunks("T", "/p", &turns(lines, true))
        .iter()
        .map(|c| hash(&c.text))
        .collect();
    assert!(after.len() > before.len());
    for h in &before {
        assert!(after.contains(h));
    }
}

#[test]
fn hash_is_a_sha256_prefix() {
    let h = hash("abc");
    assert_eq!(
        h,
        [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23
        ]
    );
    assert_ne!(hash("abc"), hash("abd"));
}

#[test]
fn page_sections_split_at_headings() {
    let html = "<html><head><title>T</title><style>.x{color:red}</style></head><body>\
        <p>intro &amp; more</p><h1>Alpha</h1><p>first body</p>\
        <script>var SECRET = '<h2>no</h2>';</script><h2>Beta</h2><p>second\n body</p>\
        <!-- hidden --><h3>Gamma</h3></body></html>";
    let chunks = page_chunks(html);
    let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(texts.len(), 4, "{texts:?}");
    assert!(texts[0].contains("intro & more"));
    assert!(texts[1].starts_with("Alpha"));
    assert!(texts[1].contains("first body"));
    assert!(texts[2].starts_with("Beta"));
    assert!(texts[2].contains("second body"));
    assert_eq!(texts[3], "Gamma");
    for t in &texts {
        assert!(!t.contains('<') && !t.contains("SECRET") && !t.contains("hidden"));
        assert!(!t.contains("color:red"));
    }
    assert!(chunks.iter().all(|c| c.kind == ChunkKind::Page));
    assert!(page_chunks("<script>x</script>  ").is_empty());
}

#[test]
fn long_page_sections_window() {
    let html = format!("<h1>Big</h1><p>{}</p>", sentences(120));
    let chunks = page_chunks(&html);
    assert!(chunks.len() >= 3);
    assert!(chunks.iter().all(|c| c.text.chars().count() <= 1200));
}

#[test]
fn cuts_never_split_a_character() {
    let unit = "Đăng nhập không được, hãy thử lại sau ít phút nữa nhé. ";
    for n in (1..60).chain([200, 400, 777, 1500]) {
        let text: String = unit.chars().cycle().take(n * 7).collect();
        let t = vec![turn("hỏi: đăng nhập lỗi", &text)];
        let chunks = session_chunks("Tiêu đề", "/dự/án", &t);
        for c in &chunks {
            assert!(c.text.chars().count() <= 1200);
            assert!(!c.text.contains('\u{fffd}'));
        }
        let last = chunks.last().unwrap();
        if n * 7 < 4000 {
            let want: String = text.trim_end().chars().rev().take(7).collect();
            let got: String = last.text.chars().rev().take(7).collect();
            assert_eq!(want, got);
        }
    }
    let unbroken: String = "ệ".repeat(5000);
    let t = vec![turn("q", &unbroken)];
    let chunks = session_chunks("", "", &t);
    assert!(chunks.iter().all(|c| c.text.chars().count() <= 1200));
    assert!(chunks.iter().any(|c| c.text.contains('ệ')));
}
