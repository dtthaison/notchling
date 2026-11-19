use super::ledger::{Call, ROLLUP_DAYS, ROW_DAYS};
use super::ledger_testkit::{line, Use, World, DAY, HOUR, NONE, NOW};
use super::summary::day_key;
use std::time::Duration;

const OPUS: &str = "claude-opus-5-5";
const SONNET: &str = "claude-sonnet-5-5";

fn use_of(input: u64, output: u64, read: u64, w5: u64, w1: u64) -> Use {
    Use {
        input,
        output,
        read,
        w5,
        w1,
        ..NONE
    }
}

fn rows(world: &World) -> Vec<Call> {
    let mut ledger = world.ledger();
    ledger.refresh(NOW, None);
    ledger.calls_since(0.0)
}

#[test]
fn a_claude_call_becomes_a_row_of_ids_and_counts() {
    let world = World::new("ledger-claude");
    world.main(
        "s1",
        &line(
            "m1",
            NOW - HOUR,
            OPUS,
            use_of(10, 200, 5_000, 0, 30_000),
            r#","effort":"medium""#,
        ),
    );
    world.sub(
        "s1",
        "agent-a1",
        r#"{"agentType":"builder","model":"sonnet","effort":"high","description":"SECRET-DESCRIPTION"}"#,
        &line(
            "m2",
            NOW - 600.0,
            SONNET,
            use_of(1, 50, 1_000, 2_000, 0),
            "",
        ),
    );
    let found = rows(&world);
    assert_eq!(found.len(), 2);
    let main = found.iter().find(|c| !c.sub).expect("main row");
    assert_eq!(main.tool, "Claude Code");
    assert_eq!(main.session, "s1");
    assert_eq!(main.agent, "");
    assert_eq!(main.model, OPUS);
    assert_eq!(main.effort.as_deref(), Some("medium"));
    assert_eq!(
        (
            main.input,
            main.output,
            main.cache_read,
            main.write_5m,
            main.write_1h
        ),
        (10, 200, 5_000, 0, 30_000)
    );
    assert!(main.split);
    assert!(main.cost.expect("priced") > 0.0);
    let sub = found.iter().find(|c| c.sub).expect("sub row");
    assert_eq!(sub.session, "s1", "the parent's id, from the folder");
    assert_eq!(sub.agent, "agent-a1");
    assert_eq!(sub.agent_type.as_deref(), Some("builder"));
    assert_eq!(sub.effort.as_deref(), Some("high"));
}

#[test]
fn no_text_of_any_kind_reaches_the_database() {
    let world = World::new("ledger-private");
    world.main(
        "s1",
        &line("m1", NOW - HOUR, OPUS, use_of(10, 20, 30, 40, 0), ""),
    );
    world.sub(
        "s1",
        "agent-a1",
        r#"{"agentType":"builder","description":"SECRET-DESCRIPTION"}"#,
        &line("m2", NOW - 60.0, SONNET, use_of(1, 1, 1, 1, 0), ""),
    );
    let mut ledger = world.file_ledger();
    ledger.refresh(NOW, None);
    drop(ledger);
    let dir = world.tmp.path("data/daemon");
    let mut all = Vec::new();
    for entry in std::fs::read_dir(dir).expect("dir").flatten() {
        all.extend(std::fs::read(entry.path()).expect("read"));
    }
    let text = String::from_utf8_lossy(&all);
    for secret in ["SECRET-REPLY-TEXT", "SECRET-COMMAND", "SECRET-DESCRIPTION"] {
        assert!(!text.contains(secret), "{secret} is in the database");
    }
    assert!(text.contains("agent-a1"));
}

#[test]
fn a_second_pass_reads_only_what_was_added_and_counts_a_message_once() {
    let world = World::new("ledger-incremental");
    let first = line("m1", NOW - 3.0 * HOUR, OPUS, use_of(1, 1, 1, 1, 0), "");
    world.main("s1", &first);
    let mut ledger = world.ledger();
    assert_eq!(ledger.refresh(NOW, None).added, 1);
    let again = ledger.refresh(NOW, None);
    assert_eq!((again.added, again.files), (0, 1));
    world.main(
        "s1",
        &line("m2", NOW - 2.0 * HOUR, OPUS, use_of(1, 1, 1, 1, 0), ""),
    );
    world.main("s2", &first);
    let third = ledger.refresh(NOW, None);
    assert_eq!(third.added, 1, "m1 copied into a new transcript is not new");
    assert_eq!(ledger.calls_since(0.0).len(), 2);
}

#[test]
fn a_budget_stops_between_files_and_the_next_pass_finishes() {
    let world = World::new("ledger-budget");
    for n in 0..4 {
        world.main(
            &format!("s{n}"),
            &line(
                &format!("m{n}"),
                NOW - HOUR,
                OPUS,
                use_of(1, 1, 1, 1, 0),
                "",
            ),
        );
    }
    let mut ledger = world.ledger();
    let cut = ledger.refresh(NOW, Some(Duration::ZERO));
    assert!(!cut.complete());
    assert_eq!(cut.left, 4);
    let rest = ledger.refresh(NOW, None);
    assert!(rest.complete());
    assert_eq!(ledger.call_count(), 4);
}

#[test]
fn rows_older_than_thirty_days_are_never_kept_and_rollups_outlive_rows() {
    let world = World::new("ledger-retention");
    let old = NOW - (ROW_DAYS as f64 + 2.0) * DAY;
    world.main(
        "s1",
        &(line("old", old, OPUS, use_of(1, 1, 1, 1, 0), "")
            + &line("new", NOW - 2.0 * DAY, OPUS, use_of(7, 1, 1, 1, 0), "")),
    );
    let mut ledger = world.ledger();
    ledger.refresh(NOW, None);
    let kept = ledger.calls_since(0.0);
    assert_eq!(kept.len(), 1, "the 32-day-old call is dropped at the door");
    assert_eq!(kept[0].input, 7);

    let later = NOW + 29.0 * DAY;
    ledger.prune(later);
    assert_eq!(ledger.call_count(), 0, "rows expire after 30 days");
    let rolled = ledger.rollups_since("0000-00-00");
    assert_eq!(rolled.len(), 1, "the day's rollup stays");
    assert_eq!((rolled[0].calls, rolled[0].input), (1, 7));
    assert_eq!(rolled[0].day, day_key(ledger.day_number(NOW - 2.0 * DAY)));
}

#[test]
fn rollups_expire_after_a_year() {
    let world = World::new("ledger-rollup-year");
    world.main(
        "s1",
        &line("m1", NOW - DAY, OPUS, use_of(1, 1, 1, 1, 0), ""),
    );
    let mut ledger = world.ledger();
    ledger.refresh(NOW, None);
    ledger.prune(NOW + (ROLLUP_DAYS as f64 - 10.0) * DAY);
    assert_eq!(ledger.rollups_since("0000-00-00").len(), 1);
    ledger.prune(NOW + (ROLLUP_DAYS as f64 + 5.0) * DAY);
    assert!(ledger.rollups_since("0000-00-00").is_empty());
}

#[test]
fn rollups_add_up_the_days_calls_by_tool_role_and_model() {
    let world = World::new("ledger-rollup");
    let t = NOW - HOUR;
    world.main(
        "s1",
        &(line("a", t, OPUS, use_of(1, 2, 3, 4, 0), "")
            + &line("b", t + 1.0, OPUS, use_of(10, 20, 30, 40, 0), "")),
    );
    world.sub(
        "s1",
        "agent-x",
        "",
        &line("c", t + 2.0, OPUS, use_of(5, 5, 5, 5, 0), ""),
    );
    let mut ledger = world.ledger();
    ledger.refresh(NOW, None);
    let rolled = ledger.rollups_since("0000-00-00");
    assert_eq!(rolled.len(), 2);
    let main = rolled.iter().find(|r| !r.sub).expect("main");
    assert_eq!((main.calls, main.input, main.output), (2, 11, 22));
    assert_eq!(main.write_5m, 44);
    assert!(main.cost > 0.0);
    assert_eq!(main.unpriced, 0);
}

#[test]
fn a_model_with_no_price_has_no_cost_not_a_zero() {
    let world = World::new("ledger-unpriced");
    world.main(
        "s1",
        &line(
            "m1",
            NOW - HOUR,
            "claude-mystery-9",
            use_of(1, 2, 3, 4, 0),
            "",
        ),
    );
    let mut ledger = world.ledger();
    ledger.refresh(NOW, None);
    let found = ledger.calls_since(0.0);
    assert_eq!(found[0].cost, None);
    assert_eq!(ledger.rollups_since("0000-00-00")[0].unpriced, 1);
}

#[test]
fn a_transcript_without_the_split_says_so() {
    let world = World::new("ledger-nosplit");
    let mut u = use_of(1, 1, 1, 90_000, 0);
    u.split = false;
    world.main("s1", &line("m1", NOW - HOUR, OPUS, u, ""));
    let found = rows(&world);
    assert!(!found[0].split, "the cache time cannot be read from it");
    assert_eq!(found[0].write_5m, 90_000);
}

#[test]
fn codex_and_antigravity_rows_come_from_the_same_sources() {
    let world = World::new("ledger-others");
    world.append(
        "codex/sessions/2026/11/06/rollout-2026-11-06T00-49-31-01a1ae55-0254-7822-814c-d6aed2cc872e.jsonl",
        include_str!("fixtures/codex-rollout.jsonl"),
    );
    world.append(
        "brain/conv-1/transcript.jsonl",
        include_str!("fixtures/antigravity-transcript.jsonl"),
    );
    let mut ledger = world.ledger();
    ledger.refresh(1_794_000_000.0, None);
    let found = ledger.calls_since(0.0);
    let codex: Vec<_> = found.iter().filter(|c| c.tool == "Codex").collect();
    let agy: Vec<_> = found.iter().filter(|c| c.tool == "Antigravity").collect();
    assert_eq!(codex.len(), 2, "the repeated running total is one call");
    assert_eq!(agy.len(), 2);
    assert_eq!(codex[0].session, "01a1ae55-0254-7822-814c-d6aed2cc872e");
    assert_eq!(codex[0].model, "gpt-5.1");
    assert!(!codex[0].sub);
    assert_eq!(agy[0].session, "conv-1");
    assert_eq!(agy[0].model, "Gemini 4 Argon");
    assert!(agy.iter().all(|c| c.write_1h == 0 && !c.split));
}
