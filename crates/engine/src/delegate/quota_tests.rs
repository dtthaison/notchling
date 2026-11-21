use super::*;
use crate::accounts::antigravity::quota::parse_usage;
use crate::providers::usage::UsageReport;

const NOW: f64 = 1_794_400_000.0;
const CARD: &str = include_str!("testdata/agy-card.json");

fn card() -> Quota {
    let usage = parse_usage(CARD.as_bytes(), NOW).unwrap();
    Quota::of(NOW, true, Some(&UsageReport::from(&usage)), None)
}

fn pool_at(q: &mut Quota, name: &str, label: &str, percent: f64) {
    let pool = q.pools.iter_mut().find(|p| p.name == name).unwrap();
    let window = pool.windows.iter_mut().find(|w| w.label == label).unwrap();
    window.percent = percent;
}

fn agy(q: &Quota, model: &str) -> Verdict {
    verdict(Some(q), Tool::Agy, model, NOW)
}

fn out(v: Verdict) -> Out {
    match v {
        Verdict::Out(o) => o,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_card_has_room_in_gemini_models_and_none_in_argon() {
    let q = card();
    assert_eq!(agy(&q, "gemini-4-hydrogen"), Verdict::Room);
    assert_eq!(agy(&q, "gemini-4-hydrogen-xhigh"), Verdict::Room);
    assert_eq!(agy(&q, "gemini-3.8-flash"), Verdict::Room);
    let argon = out(agy(&q, "gemini-4-argon"));
    assert_eq!(argon.pool, "Gemini 4 Argon");
    assert_eq!(argon.reason(NOW), "week quota used up, resets in 7d 0h");
    assert_eq!(
        note("gemini-4-argon", &argon, NOW),
        "gemini-4-argon: week quota used up, resets in 7d 0h"
    );
}

#[test]
fn claude_and_gpt_models_share_one_pool_and_95_is_the_edge() {
    let mut q = card();
    for model in ["claude-opus-5-5", "claude-sonnet-5-5", "gpt-oss-120b"] {
        assert_eq!(agy(&q, model), Verdict::Room, "5h at 94%");
    }
    pool_at(&mut q, "Claude and GPT models", "5h", 95.0);
    for model in ["claude-opus-5-5", "gpt-oss-120b"] {
        let o = out(agy(&q, model));
        assert_eq!(o.pool, "Claude and GPT models");
        assert_eq!(o.reason(NOW), "5-hour quota used up, resets in 5h 28m");
    }
    assert_eq!(agy(&q, "gemini-4-hydrogen"), Verdict::Room);
}

#[test]
fn a_gemini_pool_that_is_full_leaves_argon_and_claude_alone() {
    let mut q = card();
    pool_at(&mut q, "Gemini Models", "Week", 100.0);
    assert!(matches!(agy(&q, "gemini-4-hydrogen"), Verdict::Out(_)));
    assert!(matches!(
        agy(&q, "gemini-4-hydrogen-xhigh"),
        Verdict::Out(_)
    ));
    assert_eq!(agy(&q, "claude-opus-5-5"), Verdict::Room);
    assert!(matches!(agy(&q, "gemini-4-argon"), Verdict::Out(_)));
}

#[test]
fn a_window_that_has_reset_or_a_stale_or_missing_read_is_room() {
    let mut q = card();
    assert!(matches!(agy(&q, "gemini-4-argon"), Verdict::Out(_)));
    let later = NOW + 8.0 * 86_400.0;
    assert_eq!(
        verdict(Some(&q), Tool::Agy, "gemini-4-argon", later),
        Verdict::Room
    );
    assert_eq!(
        verdict(None, Tool::Agy, "gemini-4-argon", NOW),
        Verdict::Room
    );
    q.fetched = NOW - MAX_AGE_SECS - 1.0;
    assert_eq!(agy(&q, "gemini-4-argon"), Verdict::Room);
    assert_eq!(agy(&card(), "some-other-model"), Verdict::Room);
}

#[test]
fn codex_is_out_of_credits_or_at_its_limit_or_signed_out() {
    let credits = Quota {
        fetched: NOW,
        signed_in: true,
        reached: Some(Reached::Credits),
        pools: Vec::new(),
    };
    let o = out(verdict(Some(&credits), Tool::Codex, "gpt-6-astra", NOW));
    assert_eq!(o.reason(NOW), "out of credits");
    let limit = Quota {
        reached: Some(Reached::Limit),
        ..credits.clone()
    };
    assert_eq!(
        out(verdict(Some(&limit), Tool::Codex, "gpt-6-sol", NOW)).what,
        "limit reached"
    );
    let week = Quota {
        reached: None,
        pools: vec![Pool {
            name: String::new(),
            windows: vec![Window {
                label: "Week".into(),
                percent: 97.0,
                resets_at: Some(NOW + 3_700.0),
            }],
        }],
        ..credits.clone()
    };
    assert_eq!(
        out(verdict(Some(&week), Tool::Codex, "gpt-6-sol", NOW)).reason(NOW),
        "week quota used up, resets in 1h 1m"
    );
    let fine = Quota {
        reached: None,
        pools: Vec::new(),
        ..credits.clone()
    };
    assert_eq!(
        verdict(Some(&fine), Tool::Codex, "gpt-6-sol", NOW),
        Verdict::Room
    );
    let signed_out = Quota {
        signed_in: false,
        ..fine
    };
    assert_eq!(
        verdict(Some(&signed_out), Tool::Codex, "gpt-6-sol", NOW),
        Verdict::SignedOut
    );
}

#[test]
fn the_wait_is_short_and_plain() {
    assert_eq!(until(NOW + 30.0, NOW), "under a minute");
    assert_eq!(until(NOW + 600.0, NOW), "10m");
    assert_eq!(until(NOW + 7_500.0, NOW), "2h 5m");
    assert_eq!(until(NOW + 3.0 * 86_400.0 + 4.0 * 3_600.0, NOW), "3d 4h");
    assert_eq!(until(NOW - 5.0, NOW), "under a minute");
}

#[test]
fn the_pool_of_a_model() {
    assert_eq!(
        pool_name(Tool::Agy, "gemini-4-argon"),
        Some("Gemini 4 Argon")
    );
    assert_eq!(
        pool_name(Tool::Agy, "gemini-4-hydrogen-xhigh"),
        Some("Gemini Models")
    );
    assert_eq!(
        pool_name(Tool::Agy, "claude-opus-5-5"),
        Some("Claude and GPT models")
    );
    assert_eq!(
        pool_name(Tool::Agy, "gpt-oss-120b"),
        Some("Claude and GPT models")
    );
    assert_eq!(pool_name(Tool::Agy, "mystery"), None);
    assert_eq!(pool_name(Tool::Codex, "gpt-6-astra"), Some("Codex"));
}

#[test]
fn a_read_goes_to_a_file_and_back_and_a_bad_file_is_nothing() {
    let dir = crate::delegate::test_support::scratch("quota-file");
    assert_eq!(load(&dir, Tool::Agy), None);
    let q = card();
    save(&dir, "antigravity", &q);
    assert_eq!(load(&dir, Tool::Agy), Some(q));
    assert_eq!(load(&dir, Tool::Codex), None);
    std::fs::write(file_of(&dir, "codex"), "{nope").unwrap();
    assert_eq!(load(&dir, Tool::Codex), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_refusal_names_the_pool_and_the_reset() {
    let q = card();
    let o = out(agy(&q, "gemini-4-argon"));
    let text = refusal(Tool::Agy, Some("gemini-4-argon"), &o, NOW);
    assert!(
        text.contains("Gemini 4 Argon: week quota used up, resets in 7d 0h"),
        "{text}"
    );
    assert!(text.contains("delegate-models --for"), "{text}");
}

const QUOTA_LINE: &str = "\u{26a0} Individual quota reached. Please upgrade your subscription\u{2026} Resets in 134h28m59s";

#[test]
fn the_quota_line_antigravity_prints_is_detected_with_its_wait() {
    let found = detect(QUOTA_LINE).unwrap();
    assert_eq!(found.label, "individual");
    assert_eq!(found.resets_in, Some(134.0 * 3600.0 + 28.0 * 60.0 + 59.0));
    let no_wait = detect("Quota exceeded for this model.").unwrap();
    assert_eq!(no_wait.resets_in, None);
    assert_eq!(parse_wait("2d 3h"), Some(2.0 * 86_400.0 + 3.0 * 3_600.0));
    assert_eq!(parse_wait("soon"), None);
}

#[test]
fn an_ordinary_reply_is_never_a_quota_message() {
    assert_eq!(detect("Looks fine. No issues found."), None);
    assert_eq!(detect(""), None);
    let long = format!("{QUOTA_LINE}\n{}", "a review ".repeat(300));
    assert_eq!(detect(&long), None, "a real review that quotes the line");
}

#[test]
fn a_said_out_pool_is_marked_so_the_next_check_refuses_it() {
    let dir = crate::delegate::test_support::scratch("quota-mark");
    let source = Source {
        dir: dir.clone(),
        clock: || NOW,
    };
    let found = detect(QUOTA_LINE).unwrap();
    mark_out(&source, Tool::Agy, Some("gemini-4-hydrogen"), &found);
    let q = source.load(Tool::Agy).unwrap();
    match verdict(Some(&q), Tool::Agy, "gemini-4-hydrogen", NOW) {
        Verdict::Out(o) => {
            assert_eq!(o.pool, "Gemini Models");
            assert_eq!(o.reason(NOW), "individual quota used up, resets in 5d 14h");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        verdict(Some(&q), Tool::Agy, "gemini-4-argon", NOW),
        Verdict::Room,
        "another pool is not touched"
    );
    mark_out(&source, Tool::Agy, None, &found);
    assert_eq!(
        source.load(Tool::Agy).unwrap(),
        q,
        "no model, no pool to mark"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn codex_quota() -> Quota {
    Quota {
        fetched: NOW,
        signed_in: true,
        reached: None,
        pools: vec![Pool {
            name: "Codex".into(),
            windows: vec![Window {
                label: "5h".into(),
                percent: 10.0,
                resets_at: Some(NOW + 3600.0),
            }],
        }],
    }
}

#[test]
fn readiness_is_usable_when_all_is_fine() {
    assert!(readiness(Some(&codex_quota()), Tool::Codex, NOW).usable);
    let ready = readiness(Some(&card()), Tool::Agy, NOW);
    assert!(ready.usable);
    assert_eq!(ready.reason, None);
    assert_eq!(ready.age_secs, Some(0.0));
    assert!(readiness(None, Tool::Codex, NOW).usable);
}

#[test]
fn readiness_says_credits_out_when_codex_credits_are_out() {
    let mut q = codex_quota();
    q.reached = Some(Reached::Credits);
    let ready = readiness(Some(&q), Tool::Codex, NOW);
    assert!(!ready.usable);
    assert_eq!(ready.reason, Some("credits_out"));
    assert!(ready.pools.iter().all(|p| !p.usable));
}

#[test]
fn readiness_says_pool_full_at_96_percent() {
    let mut q = codex_quota();
    q.pools[0].windows[0].percent = 96.0;
    let ready = readiness(Some(&q), Tool::Codex, NOW);
    assert!(!ready.usable);
    assert_eq!(ready.reason, Some("pool_full"));
    assert_eq!(ready.pools[0].reason, Some("pool_full"));
    q.pools[0].windows[0].percent = 94.0;
    assert!(readiness(Some(&q), Tool::Codex, NOW).usable);
}

#[test]
fn readiness_of_agy_is_per_pool() {
    let mut q = card();
    let first = q.pools[0].name.clone();
    let label = q.pools[0].windows[0].label.clone();
    pool_at(&mut q, &first, &label, 96.0);
    let ready = readiness(Some(&q), Tool::Agy, NOW);
    assert!(!ready.pools.iter().find(|p| p.name == first).unwrap().usable);
    assert_eq!(ready.usable, ready.pools.iter().any(|p| p.usable));
}

#[test]
fn readiness_says_stale_and_signed_out() {
    let mut q = codex_quota();
    q.fetched = NOW - MAX_AGE_SECS - 1.0;
    assert_eq!(readiness(Some(&q), Tool::Codex, NOW).reason, Some("stale"));
    q.fetched = NOW;
    q.signed_in = false;
    assert_eq!(
        readiness(Some(&q), Tool::Codex, NOW).reason,
        Some("signed_out")
    );
}

#[test]
fn a_reached_credit_or_limit_counts_for_every_tool() {
    for (reached, reason) in [
        (Reached::Credits, "credits_out"),
        (Reached::Limit, "limit_reached"),
    ] {
        let mut q = card();
        q.reached = Some(reached);
        let ready = readiness(Some(&q), Tool::Agy, NOW);
        assert!(!ready.usable);
        assert_eq!(ready.reason, Some(reason));
    }
}

#[test]
fn a_signed_out_account_with_no_reading_is_not_usable() {
    let ready = readiness_of_windows(None, false, Vec::new(), NOW);
    assert!(!ready.usable);
    assert_eq!(ready.reason, Some("signed_out"));
    assert!(readiness_of_windows(None, true, Vec::new(), NOW).usable);
}
