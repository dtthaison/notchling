use crate::accounts::antigravity::quota::parse_usage;
use crate::delegate::command::{models_listing, models_listing_json};
use crate::delegate::quota::{self, Quota, Source};
use crate::delegate::test_support::{rig, Rig};
use crate::providers::usage::{Reached, UsageReport};
use crate::tools::acp::fake::FakeSpawner as AcpFake;
use std::sync::Arc;

const NOW: f64 = 1_794_400_000.0;
const CARD: &str = include_str!("testdata/agy-card.json");
const CODEX: &str = r#"{"models":[{"slug":"gpt-6-astra","priority":1,"visibility":"list"},
  {"slug":"gpt-6-sol","priority":2,"visibility":"list"}]}"#;
const AGY: &str = "gemini-4-hydrogen-high\tH\ngemini-4-hydrogen-xhigh\tX\ngemini-4-argon-high\tA\nclaude-opus-5-5-high\tC\n";

fn clock() -> f64 {
    NOW
}

fn setup() -> Rig {
    let mut rig = rig(AcpFake::new());
    std::fs::create_dir_all(&rig.deps.codex_home).unwrap();
    std::fs::write(rig.deps.codex_home.join("models_cache.json"), CODEX).unwrap();
    rig.deps.capture = Arc::new(|_, _| Some(AGY.to_string()));
    rig.deps.quota = Some(Source {
        dir: rig.root.join("quota"),
        clock,
    });
    rig
}

fn room() -> Quota {
    let mut q = card();
    for pool in &mut q.pools {
        for w in &mut pool.windows {
            w.percent = 0.0;
        }
    }
    q
}

fn card() -> Quota {
    let usage = parse_usage(CARD.as_bytes(), NOW).unwrap();
    Quota::of(NOW, true, Some(&UsageReport::from(&usage)), None)
}

fn codex(reached: Option<Reached>) -> Quota {
    Quota {
        fetched: NOW,
        signed_in: true,
        reached,
        pools: Vec::new(),
    }
}

fn put(rig: &Rig, provider: &str, q: &Quota) {
    quota::save(&rig.root.join("quota"), provider, q);
}

fn set(q: &mut Quota, pool: &str, label: &str, percent: f64) {
    let p = q.pools.iter_mut().find(|p| p.name == pool).unwrap();
    p.windows
        .iter_mut()
        .find(|w| w.label == label)
        .unwrap()
        .percent = percent;
}

fn pick(rig: &Rig, role: &str) -> String {
    let mut out = Vec::new();
    let args = vec!["--for".to_string(), role.to_string()];
    assert_eq!(models_listing(&args, &rig.deps, &mut out), 0);
    String::from_utf8(out).unwrap()
}

#[test]
fn with_room_everywhere_the_first_picks_stand_and_nothing_is_said() {
    let rig = setup();
    put(&rig, "antigravity", &room());
    put(&rig, "codex", &codex(None));
    assert_eq!(
        pick(&rig, "review"),
        "agy --model gemini-4-hydrogen --effort high\n"
    );
    assert_eq!(
        pick(&rig, "review-final"),
        "codex --model gpt-6-astra --effort xhigh\n"
    );
    assert_eq!(
        pick(&rig, "review-secrets"),
        "codex --model gpt-6-astra --effort xhigh\nagy --model gemini-4-argon --effort high\n"
    );
    assert_eq!(
        pick(&rig, "spec-check"),
        "agy --model gemini-4-argon --effort high\n"
    );
}

#[test]
fn unknown_quota_counts_as_room() {
    let rig = setup();
    assert_eq!(
        pick(&rig, "spec-check"),
        "agy --model gemini-4-argon --effort high\n"
    );
    put(
        &rig,
        "antigravity",
        &Quota {
            fetched: NOW - 4.0 * 3600.0,
            ..card()
        },
    );
    assert_eq!(
        pick(&rig, "spec-check"),
        "agy --model gemini-4-argon --effort high\n"
    );
}

#[test]
fn argon_out_moves_the_spec_check_and_the_second_secrets_reviewer() {
    let rig = setup();
    put(&rig, "antigravity", &card());
    put(&rig, "codex", &codex(None));
    assert_eq!(
        pick(&rig, "spec-check"),
        "agy --model gemini-4-hydrogen-xhigh\nnote: gemini-4-argon: week quota used up, resets in 7d 0h\n"
    );
    assert_eq!(
        pick(&rig, "review-secrets"),
        "codex --model gpt-6-astra --effort xhigh\nagy --model claude-opus-5-5 --effort high\nnote: gemini-4-argon: week quota used up, resets in 7d 0h\n"
    );
    assert_eq!(
        pick(&rig, "review"),
        "agy --model gemini-4-hydrogen --effort high\n"
    );
}

#[test]
fn codex_out_of_credits_moves_the_final_review_to_antigravity() {
    let rig = setup();
    put(&rig, "antigravity", &room());
    put(&rig, "codex", &codex(Some(Reached::Credits)));
    assert_eq!(
        pick(&rig, "review-final"),
        "agy --model gemini-4-hydrogen-xhigh\nnote: gpt-6-astra: out of credits\n"
    );
}

#[test]
fn the_two_secrets_reviewers_come_from_different_families_when_they_can() {
    let rig = setup();
    put(&rig, "antigravity", &room());
    put(&rig, "codex", &codex(Some(Reached::Credits)));
    assert_eq!(
        pick(&rig, "review-secrets"),
        "agy --model gemini-4-hydrogen-xhigh\nagy --model claude-opus-5-5 --effort high\nnote: gpt-6-astra: out of credits\n"
    );
    let mut q = room();
    set(&mut q, "Claude and GPT models", "Week", 99.0);
    put(&rig, "antigravity", &q);
    assert_eq!(
        pick(&rig, "review-secrets"),
        "agy --model gemini-4-hydrogen-xhigh\nagy --model gemini-4-argon --effort high\nnote: gpt-6-astra: out of credits\n"
    );
}

#[test]
fn the_claude_and_gpt_five_hour_pool_at_94_has_room_and_at_95_it_does_not() {
    let rig = setup();
    put(&rig, "antigravity", &card());
    put(&rig, "codex", &codex(Some(Reached::Credits)));
    let at_94 = pick(&rig, "review-final");
    assert_eq!(
        at_94,
        "agy --model gemini-4-hydrogen-xhigh\nnote: gpt-6-astra: out of credits\n"
    );
    let mut q = card();
    set(&mut q, "Gemini Models", "Week", 97.0);
    put(&rig, "antigravity", &q);
    assert_eq!(
        pick(&rig, "review-final"),
        "agy --model claude-opus-5-5 --effort high\nnote: gpt-6-astra: out of credits\nnote: gemini-4-hydrogen-xhigh: week quota used up, resets in 7d 0h\n"
    );
    set(&mut q, "Claude and GPT models", "5h", 95.0);
    put(&rig, "antigravity", &q);
    assert_eq!(
        pick(&rig, "review-final"),
        "codex --model gpt-6-astra --effort xhigh\nnote: gpt-6-astra: out of credits\nnote: gemini-4-hydrogen-xhigh: week quota used up, resets in 7d 0h\nnote: claude-opus-5-5: 5-hour quota used up, resets in 5h 28m\n"
    );
}

#[test]
fn a_full_gemini_pool_sends_the_review_to_codex() {
    let rig = setup();
    let mut q = room();
    set(&mut q, "Gemini Models", "5h", 100.0);
    put(&rig, "antigravity", &q);
    put(&rig, "codex", &codex(None));
    assert_eq!(
        pick(&rig, "review"),
        "codex --model gpt-6-sol --effort high\nnote: gemini-4-hydrogen: 5-hour quota used up, resets in 5h 28m\nnote: gemini-4-hydrogen-xhigh: 5-hour quota used up, resets in 5h 28m\n"
    );
}

#[test]
fn a_tool_that_is_signed_out_is_skipped() {
    let rig = setup();
    put(&rig, "antigravity", &room());
    put(
        &rig,
        "codex",
        &Quota {
            signed_in: false,
            ..codex(None)
        },
    );
    assert_eq!(
        pick(&rig, "review-final"),
        "agy --model gemini-4-hydrogen-xhigh\nnote: gpt-6-astra: codex is signed out\n"
    );
}

#[test]
fn when_no_pick_has_room_the_first_comes_back_with_the_reasons() {
    let rig = setup();
    let mut q = card();
    set(&mut q, "Gemini Models", "Week", 100.0);
    set(&mut q, "Claude and GPT models", "Week", 100.0);
    put(&rig, "antigravity", &q);
    put(&rig, "codex", &codex(Some(Reached::Credits)));
    let text = pick(&rig, "review-final");
    assert!(
        text.starts_with(
            "codex --model gpt-6-astra --effort xhigh\nnote: gpt-6-astra: out of credits\n"
        ),
        "{text}"
    );
    assert_eq!(text.lines().count(), 4);
    assert_eq!(
        pick(&rig, "spec-check").lines().next(),
        Some("agy --model gemini-4-argon --effort high")
    );
}

#[test]
fn json_has_the_picks_without_an_empty_effort_and_the_notes() {
    let rig = setup();
    put(&rig, "antigravity", &card());
    let mut out = Vec::new();
    let args = vec!["--for".to_string(), "spec-check".to_string()];
    assert_eq!(models_listing_json(&args, &rig.deps, &mut out), 0);
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v["picks"],
        serde_json::json!([{"tool": "agy", "model": "gemini-4-hydrogen-xhigh"}])
    );
    assert_eq!(
        v["notes"],
        serde_json::json!(["gemini-4-argon: week quota used up, resets in 7d 0h"])
    );
}
