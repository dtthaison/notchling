//! Port of `CostSourcesTests`' parser half, and the price tables. The fixtures have the real
//! files' shape with made-up numbers.

use super::other::{antigravity, codex, OtherPrices};
use super::prices;
use super::tracker::{Tracker, ANTIGRAVITY, CLAUDE_CODE, CODEX};
use serde_json::{json, Value};

const CODEX_ROLLOUT: &str = include_str!("fixtures/codex-rollout.jsonl");
const AGY_TRANSCRIPT: &str = include_str!("fixtures/antigravity-transcript.jsonl");

fn objects(text: &str) -> Vec<Value> {
    text.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

#[test]
fn codex_events() {
    let events: Vec<_> = objects(CODEX_ROLLOUT)
        .iter()
        .filter_map(codex::parse)
        .collect();
    assert_eq!(
        events.len(),
        5,
        "session, turn and three token_count lines with info; the null-info one and agent_message are skipped"
    );
    assert_eq!(
        events[0],
        codex::Event::Session {
            account: Some("user-AAA:acct-111".into())
        }
    );
    assert_eq!(
        events[1],
        codex::Event::Turn {
            model: Some("gpt-5.1".into()),
            folder: Some("/Users/me/work/demo".into())
        }
    );
    let codex::Event::Tokens(second) = &events[3] else {
        panic!("not tokens");
    };
    assert_eq!(second.input, 25_000.0);
    assert_eq!(second.cached, 20_000.0);
    assert_eq!(
        second.output, 200.0,
        "the per-request number, not the running total of 300"
    );
    assert_eq!(second.total, 45_300.0);
    assert_eq!(
        codex::counts(second),
        [5_000.0, 200.0, 20_000.0, 0.0],
        "input is counted without its cached part"
    );
}

#[test]
fn a_session_meta_without_a_creator_names_no_account() {
    let line = json!({"type": "session_meta", "payload": {"id": "x"}});
    assert_eq!(
        codex::parse(&line),
        Some(codex::Event::Session { account: None })
    );
}

#[test]
fn antigravity_events() {
    let events: Vec<_> = objects(AGY_TRANSCRIPT)
        .iter()
        .filter_map(antigravity::parse)
        .collect();
    assert_eq!(events.len(), 3, "the tool output step is skipped");
    assert_eq!(
        events[0],
        antigravity::Event::Model("Gemini 4 Argon".into())
    );
    let antigravity::Event::Tokens(t) = &events[2] else {
        panic!("not tokens");
    };
    assert_eq!(antigravity::counts(t), [2567.0, 365.0, 6418.0, 0.0]);
}

#[test]
fn model_selection_change() {
    assert_eq!(
        antigravity::model_in(
            "x `Model Selection` from Gemini 4 Argon (Medium) to Gemini 3.8 Flash (Low). No need to comment"
        )
        .as_deref(),
        Some("Gemini 3.8 Flash")
    );
    assert_eq!(
        antigravity::model_in("`Model Selection` from None to Plain. No need").as_deref(),
        Some("Plain")
    );
    assert_eq!(antigravity::model_in("just a prompt"), None);
}

#[test]
fn other_prices() {
    // gpt-5.1: $1.25 in, $0.125 cached, $10 out per million.
    let near = |a: f64, b: f64| assert!((a - b).abs() < 1e-9, "{a} vs {b}");
    near(
        OtherPrices::cost("gpt-5.1", &[1_000_000.0, 100_000.0, 2_000_000.0]),
        1.25 + 1.0 + 0.25,
    );
    near(
        OtherPrices::cost("gpt-5.1-2026-01-01", &[1_000_000.0, 0.0, 0.0]),
        1.25,
    );
    assert_eq!(
        OtherPrices::cost("gpt-6.1-sol", &[1_000_000.0, 1_000_000.0, 0.0]),
        0.0,
        "unknown model: no cost"
    );
    assert_eq!(
        OtherPrices::cost("gpt-5.6-sol", &[1_000_000.0, 0.0, 0.0]),
        0.0,
        "a longer id doesn't take gpt-5's price"
    );
}

#[test]
fn which_files_are_read() {
    let ok = |p: &str, c: &str| Tracker::accepts(p, c);
    assert!(ok(
        "/x/2026/11/06/rollout-2026-11-06T07-49-29-abc.jsonl",
        CODEX
    ));
    assert!(!ok(
        "/x/2026/11/06/rollout-2026-11-06T07-49-29-abc.jsonl.zst",
        CODEX
    ));
    assert!(!ok("/x/history.jsonl", CODEX));
    assert!(ok(
        "/b/1/.system_generated/logs/transcript.jsonl",
        ANTIGRAVITY
    ));
    assert!(!ok(
        "/b/1/.system_generated/logs/transcript_full.jsonl",
        ANTIGRAVITY
    ));
    assert!(ok("/p/a.jsonl", CLAUDE_CODE));
    assert!(!ok("/p/a.json", CLAUDE_CODE));
}

#[test]
fn claude_prices() {
    let near = |a: f64, b: f64| assert!((a - b).abs() < 1e-9, "{a} vs {b}");
    // Opus 5.5: $4 in, $20 out, $0.20 cache read.
    near(
        prices::cost(
            "claude-opus-5-5",
            &json!({"input_tokens": 1_000_000, "output_tokens": 1_000_000, "cache_read_input_tokens": 1_000_000}),
        ),
        4.0 + 20.0 + 0.20,
    );
    // Cache writes: 1.25x input for five minutes, 2x for an hour; the split wins over the total.
    near(
        prices::cost(
            "claude-opus-5-5",
            &json!({"cache_creation_input_tokens": 9_999_999,
                    "cache_creation": {"ephemeral_5m_input_tokens": 1_000_000, "ephemeral_1h_input_tokens": 1_000_000}}),
        ),
        4.0 * 1.25 + 4.0 * 2.0,
    );
    near(
        prices::cost(
            "claude-opus-5-5",
            &json!({"cache_creation_input_tokens": 1_000_000}),
        ),
        5.0,
    );
    // Fast mode doubles it.
    near(
        prices::cost(
            "claude-opus-5-5",
            &json!({"output_tokens": 1_000_000, "speed": "fast"}),
        ),
        40.0,
    );
    // Haiku 5.5 costs five times as much once the prompt is over 100K tokens.
    near(
        prices::cost("claude-haiku-5-5", &json!({"input_tokens": 50_000})),
        0.005,
    );
    near(
        prices::cost("claude-haiku-5-5", &json!({"input_tokens": 200_000})),
        0.1,
    );
    // Longer ids come first: opus-5-5 is not opus-5.
    near(
        prices::cost("claude-opus-5-1", &json!({"output_tokens": 1_000_000})),
        25.0,
    );
    assert_eq!(
        prices::cost("gpt-5", &json!({"output_tokens": 1_000_000})),
        0.0
    );
    assert_eq!(
        prices::cost("claude-new-9", &json!({"output_tokens": 1})),
        0.0
    );
}

#[test]
fn model_keys_and_names() {
    assert_eq!(prices::model_key("claude-opus-5-5[1m]"), "claude-opus-5-5");
    assert_eq!(
        prices::model_key("claude-haiku-4-5-20251001"),
        "claude-haiku-4-5"
    );
    assert_eq!(prices::model_key("claude-opus-5-5"), "claude-opus-5-5");
    assert_eq!(prices::display_name("claude-opus-5-5"), "Opus 5.5");
    assert_eq!(prices::display_name("claude-fable"), "Fable");
    assert_eq!(prices::display_name("Gemini 4 Argon"), "Gemini 4 Argon");
    assert_eq!(prices::display_name("gpt-6.1-sol"), "gpt-6.1-sol");
}
