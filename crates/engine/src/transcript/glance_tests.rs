use super::*;
use crate::transcript::codex;
use crate::transcript::conversation::{self, Parser};
use crate::transcript::tail::LineParser;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "glance-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        Dir(dir)
    }

    fn file(&self, name: &str, lines: &[String]) -> String {
        let path = self.0.join(name);
        let mut text = lines.join("\n");
        text.push('\n');
        std::fs::write(&path, text).expect("write");
        path.to_string_lossy().into_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn assistant(n: usize, cache_read: u64, output: u64, text: &str) -> String {
    format!(
        r#"{{"isSidechain":true,"type":"assistant","message":{{"id":"m{n}","role":"assistant","model":"claude-sonnet-5-5","content":[{{"type":"text","text":"{text}"}}],"usage":{{"input_tokens":10,"cache_read_input_tokens":{cache_read},"output_tokens":{output}}}}},"uuid":"a{n}","timestamp":"2026-11-08T08:00:05.000Z"}}"#
    )
}

fn user(n: usize, text: &str) -> String {
    format!(
        r#"{{"isSidechain":true,"type":"user","message":{{"role":"user","content":"{text}"}},"uuid":"u{n}","timestamp":"2026-11-08T08:00:01.000Z"}}"#
    )
}

fn tool_call(n: usize, command: &str) -> String {
    format!(
        r#"{{"isSidechain":true,"type":"assistant","message":{{"id":"t{n}","role":"assistant","model":"claude-sonnet-5-5","content":[{{"type":"tool_use","id":"call{n}","name":"Bash","input":{{"command":"{command}","description":"Run it"}}}}],"usage":{{"input_tokens":5,"cache_read_input_tokens":900,"output_tokens":7}}}},"uuid":"c{n}","timestamp":"2026-11-08T08:00:06.000Z"}}"#
    )
}

fn filler(count: usize, width: usize) -> Vec<String> {
    (0..count).map(|n| user(n, &"y".repeat(width))).collect()
}

fn full_context(path: &str) -> u64 {
    let mut parser = Parser::for_subagent();
    for line in std::fs::read_to_string(path).expect("read").lines() {
        parser.consume(line.as_bytes());
    }
    parser.conversation().facts.context_tokens
}

#[test]
fn a_window_is_the_end_of_the_file_without_its_cut_line() {
    let dir = Dir::new();
    let lines = filler(2000, 100);
    let path = dir.file("a.jsonl", &lines);
    let window = Window::read(&path, FIRST).expect("read");
    assert!(window.len() as u64 <= FIRST);
    assert!(!window.whole());
    let got = window.lines();
    assert_eq!(
        got.last().map(|l| l.to_vec()),
        Some(lines[1999].clone().into_bytes())
    );
    assert!(
        got.iter().all(|l| l.starts_with(b"{")),
        "the cut first line is gone"
    );

    let small = dir.file("b.jsonl", &lines[..3]);
    let whole = Window::read(&small, FIRST).expect("read");
    assert!(whole.whole());
    assert_eq!(whole.lines().len(), 3, "a whole file keeps its first line");
}

#[test]
fn an_unfinished_last_line_is_left_out() {
    let dir = Dir::new();
    let path = dir.0.join("c.jsonl");
    std::fs::write(&path, "{\"a\":1}\n{\"b\":2}\n{\"c\":").expect("write");
    let window = Window::read(&path.to_string_lossy(), FIRST).expect("read");
    assert_eq!(window.lines().len(), 2);
}

#[test]
fn the_windows_grow_to_the_cap_and_no_further() {
    let dir = Dir::new();
    let path = dir.file("d.jsonl", &filler(40_000, 100));
    let mut sizes = Vec::new();
    let out: Option<()> = widen(&path, |w| {
        sizes.push(w.len() as u64);
        ((), false)
    });
    assert!(out.is_some());
    assert_eq!(sizes.len(), 3, "{sizes:?}");
    assert!(sizes.windows(2).all(|p| p[0] < p[1]), "{sizes:?}");
    assert!(sizes.iter().all(|s| *s <= CAP), "{sizes:?}");
    assert_eq!(widen("/nonexistent/x.jsonl", |_| ((), true)), None);
}

#[test]
fn context_is_the_last_usage_a_small_tail_holds() {
    let dir = Dir::new();
    let mut lines = filler(3000, 200);
    lines.push(assistant(1, 231_000, 300, "done"));
    lines.push(user(9, "thanks"));
    let path = dir.file("e.jsonl", &lines);
    assert_eq!(conversation::subagent_context_tokens(&path), Some(231_310));
    assert_eq!(full_context(&path), 231_310);
}

#[test]
fn a_usage_further_back_widens_the_window_and_one_past_the_cap_is_not_found() {
    let dir = Dir::new();
    let mut lines = vec![assistant(1, 50_000, 10, "early")];
    lines.extend(filler(1200, 200));
    let path = dir.file("f.jsonl", &lines);
    assert_eq!(conversation::subagent_context_tokens(&path), Some(50_020));
    assert_eq!(full_context(&path), 50_020);

    let mut far = vec![assistant(1, 50_000, 10, "early")];
    far.extend(filler(6000, 200));
    let path = dir.file("g.jsonl", &far);
    assert_eq!(conversation::subagent_context_tokens(&path), None);
}

#[test]
fn an_empty_usage_a_foreign_line_and_an_oversized_line_do_not_count() {
    let dir = Dir::new();
    let big = assistant(3, 77_000, 5, &"z".repeat(450_000));
    let mut lines = vec![assistant(1, 40_000, 10, "real")];
    lines.push(big);
    lines.push(assistant(2, 0, 0, "empty").replace("\"input_tokens\":10", "\"input_tokens\":0"));
    lines.push(r#"{"type":"summary","usage":{"input_tokens":999999}}"#.to_owned());
    let path = dir.file("h.jsonl", &lines);
    assert_eq!(conversation::subagent_context_tokens(&path), Some(40_020));
    assert_eq!(full_context(&path), 40_020);
}

#[test]
fn a_compaction_after_the_last_answer_sets_the_context() {
    let dir = Dir::new();
    let lines = vec![
        assistant(1, 90_000, 10, "long"),
        r#"{"isSidechain":true,"type":"system","subtype":"compact_boundary","compactMetadata":{"postTokens":12000}}"#
            .to_owned(),
    ];
    let path = dir.file("i.jsonl", &lines);
    assert_eq!(conversation::subagent_context_tokens(&path), Some(12_000));
    assert_eq!(full_context(&path), 12_000);
}

#[test]
fn the_newest_step_of_a_small_tail_is_the_newest_step_of_the_whole_file() {
    let dir = Dir::new();
    let mut lines = filler(1500, 300);
    lines.push(assistant(1, 1000, 10, "looking"));
    lines.push(tool_call(1, "cargo test"));
    let path = dir.file("j.jsonl", &lines);
    let small = conversation::recent_subagent(&path).expect("read");
    let mut parser = Parser::for_subagent();
    for line in &lines {
        parser.consume(line.as_bytes());
    }
    let whole = parser.conversation();
    assert_eq!(
        small.items.last().map(|i| &i.kind),
        whole.items.last().map(|i| &i.kind)
    );
    assert!(small.items.len() < whole.items.len(), "it is only the tail");
    assert!(has_step(&small));
}

#[test]
fn a_tail_with_no_step_widens_until_it_has_one() {
    let dir = Dir::new();
    let mut lines = vec![user(0, "start")];
    lines.push(tool_call(1, "make"));
    lines.push(user(2, &"w".repeat(300_000)));
    let path = dir.file("k.jsonl", &lines);
    let small = conversation::recent_subagent(&path).expect("read");
    assert!(has_step(&small));
}

fn token_count(input: u64, output: u64) -> String {
    format!(
        r#"{{"timestamp":"2026-11-12T10:00:03.000Z","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":1,"output_tokens":1,"cached_input_tokens":0}},"last_token_usage":{{"input_tokens":{input},"output_tokens":{output}}}}}}}}}"#
    )
}

fn rollout_line(n: usize) -> String {
    format!(
        r#"{{"timestamp":"2026-11-12T10:00:01.000Z","type":"response_item","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"step {n} {}"}}]}}}}"#,
        "q".repeat(200)
    )
}

#[test]
fn a_rollouts_context_is_its_last_token_count_with_usage() {
    let dir = Dir::new();
    let mut lines: Vec<String> = (0..3000).map(rollout_line).collect();
    lines.insert(5, token_count(100, 1));
    lines.push(token_count(40_000, 250));
    lines.push(
        r#"{"timestamp":"2026-11-12T10:00:04.000Z","type":"event_msg","payload":{"type":"token_count","info":null}}"#
            .to_owned(),
    );
    lines.push(rollout_line(9999));
    let path = dir.file("rollout.jsonl", &lines);
    assert_eq!(codex::context_tokens(&path), Some(40_250));
    let mut parser = codex::Parser::new();
    for line in &lines {
        LineParser::consume(&mut parser, line.as_bytes());
    }
    assert_eq!(parser.conversation().facts.context_tokens, 40_250);
}

#[test]
fn a_rollout_with_no_token_count_has_no_context_and_still_has_a_tail() {
    let dir = Dir::new();
    let lines: Vec<String> = (0..50).map(rollout_line).collect();
    let path = dir.file("quiet.jsonl", &lines);
    assert_eq!(codex::context_tokens(&path), None);
    let recent = codex::recent(&path).expect("read");
    assert!(has_step(&recent));
}

fn kinds(c: &Conversation) -> Vec<String> {
    c.items.iter().map(|i| format!("{:?}", i.kind)).collect()
}

#[test]
fn one_pass_gives_the_number_and_the_tail_the_two_passes_gave() {
    let dir = Dir::new();
    let mut lines = filler(3000, 200);
    lines.push(assistant(1, 231_000, 300, "looking"));
    lines.push(tool_call(2, "cargo test"));
    let path = dir.file("l.jsonl", &lines);
    let both = conversation::peek_subagent(&path, true).expect("read");
    assert_eq!(both.context, conversation::subagent_context_tokens(&path));
    let alone = conversation::recent_subagent(&path).expect("read");
    assert_eq!(kinds(&both.recent.expect("a tail")), kinds(&alone));
    let number_only = conversation::peek_subagent(&path, false).expect("read");
    assert_eq!(number_only.context, both.context);
    assert!(number_only.recent.is_none(), "no tail unless asked for");
}

#[test]
fn a_number_far_back_widens_the_window_and_keeps_the_tail_of_the_first_one() {
    let dir = Dir::new();
    let mut lines = vec![assistant(1, 50_000, 10, "early")];
    lines.extend(filler(1200, 200));
    lines.push(user(2, "go on"));
    let path = dir.file("m.jsonl", &lines);
    let both = conversation::peek_subagent(&path, true).expect("read");
    assert_eq!(both.context, Some(50_020), "the wide window found it");
    let alone = conversation::recent_subagent(&path).expect("read");
    let tail = both.recent.expect("a tail");
    assert_eq!(kinds(&tail), kinds(&alone), "the small window had a step");
    assert!(has_step(&tail));
}

#[test]
fn a_rollout_reads_both_in_one_pass_too() {
    let dir = Dir::new();
    let mut lines: Vec<String> = (0..3000).map(rollout_line).collect();
    lines.push(token_count(40_000, 250));
    lines.push(rollout_line(9999));
    let path = dir.file("one-pass.jsonl", &lines);
    let both = codex::peek(&path, true).expect("read");
    assert_eq!(both.context, Some(40_250));
    assert_eq!(
        kinds(&both.recent.expect("a tail")),
        kinds(&codex::recent(&path).expect("read"))
    );
    assert!(codex::peek(&path, false).expect("read").recent.is_none());
}

#[test]
fn a_file_that_is_not_there_has_neither() {
    assert!(conversation::peek_subagent("/nonexistent/x.jsonl", true).is_none());
    assert!(codex::peek("/nonexistent/x.jsonl", true).is_none());
}
