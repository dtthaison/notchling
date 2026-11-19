use super::check::{analyse, ttl_of, Finding, Options, Plan, Report, Ttl};
use super::ledger::Call;
use super::ledger_testkit::{DAY, HOUR, NOW};
use super::prices;
use crate::catalog::{Catalog, Tool};

const OPUS: &str = "claude-opus-5-5";
const FABLE: &str = "claude-fable-5-2";
const SONNET: &str = "claude-sonnet-5-5";

#[derive(Clone)]
struct Spec {
    session: &'static str,
    agent: &'static str,
    agent_type: Option<&'static str>,
    model: &'static str,
    effort: Option<&'static str>,
}

fn main_of(model: &'static str) -> Spec {
    Spec {
        session: "s1",
        agent: "",
        agent_type: None,
        model,
        effort: None,
    }
}

fn sub_of(agent: &'static str, model: &'static str) -> Spec {
    Spec {
        session: "s1",
        agent,
        agent_type: Some("builder"),
        model,
        effort: None,
    }
}

fn call(spec: &Spec, ts: f64, read: u64, w5: u64, w1: u64) -> Call {
    let catalog = Catalog::built_in();
    let mut c = Call {
        ts,
        tool: "Claude Code".into(),
        session: spec.session.into(),
        agent: spec.agent.into(),
        sub: !spec.agent.is_empty(),
        agent_type: spec.agent_type.map(String::from),
        model: spec.model.into(),
        effort: spec.effort.map(String::from),
        input: 10,
        output: 300,
        cache_read: read,
        write_5m: w5,
        write_1h: w1,
        split: true,
        cost: None,
    };
    c.cost = catalog.price(Tool::Claude, spec.model).map(|p| {
        prices::estimate(
            &p,
            &[
                c.input as f64,
                c.output as f64,
                read as f64,
                w5 as f64,
                w1 as f64,
            ],
        )
    });
    c
}

fn run(calls: &[Call]) -> Report {
    analyse(calls, &Catalog::built_in(), &Options::new(NOW))
}

fn finding<'a>(report: &'a Report, id: &str) -> Option<&'a Finding> {
    report
        .findings
        .iter()
        .chain(&report.not_worth)
        .find(|f| f.id == id)
}

fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6, "{a} vs {b}");
}

fn sonnet_price() -> crate::catalog::Price {
    Catalog::built_in()
        .price(Tool::Claude, SONNET)
        .expect("price")
}

#[test]
fn the_cache_time_is_read_from_the_writes_and_never_assumed() {
    let spec = main_of(OPUS);
    let five = call(&spec, 1.0, 0, 5_000, 0);
    let hour = call(&spec, 2.0, 0, 0, 5_000);
    let mut bare = call(&spec, 3.0, 0, 5_000, 0);
    bare.split = false;
    let reads = call(&spec, 4.0, 9_000, 0, 0);
    assert_eq!(ttl_of([&five]), Ttl::FiveMin);
    assert_eq!(ttl_of([&hour]), Ttl::OneHour);
    assert_eq!(ttl_of([&five, &hour]), Ttl::Mixed);
    assert_eq!(
        ttl_of([&bare]),
        Ttl::Unknown,
        "a transcript with no split is not read as five minutes"
    );
    assert_eq!(ttl_of([&reads]), Ttl::Unknown, "no writes, no evidence");
}

#[test]
fn the_report_counts_main_sessions_and_helper_runs_by_their_measured_time() {
    let main = main_of(OPUS);
    let a = sub_of("a1", SONNET);
    let mut b = sub_of("a2", SONNET);
    b.session = "s2";
    let mut unknown = call(&b, NOW - 100.0, 1_000, 2_000, 0);
    unknown.split = false;
    let report = run(&[
        call(&main, NOW - 200.0, 0, 0, 3_000),
        call(&a, NOW - 150.0, 0, 2_000, 0),
        unknown,
    ]);
    assert_eq!(report.cache.main.one_hour, 1);
    assert_eq!(report.cache.sub.five_min, 1);
    assert_eq!(report.cache.sub.unknown, 1);
    let blind = report.unknown.iter().find(|u| u.id == "cache_ttl");
    assert_eq!(blind.map(|u| u.count), Some(1));
}

#[test]
fn opus_and_fable_on_subagents_are_priced_against_the_middle_model() {
    let mut calls = Vec::new();
    for n in 0..20 {
        calls.push(call(
            &sub_of("a1", OPUS),
            NOW - 5_000.0 + n as f64,
            50_000,
            100_000,
            0,
        ));
        calls.push(call(
            &sub_of("a2", FABLE),
            NOW - 4_000.0 + n as f64,
            50_000,
            100_000,
            0,
        ));
        calls.push(call(
            &sub_of("a3", SONNET),
            NOW - 3_000.0 + n as f64,
            50_000,
            100_000,
            0,
        ));
        calls.push(call(
            &main_of(OPUS),
            NOW - 2_000.0 + n as f64,
            50_000,
            100_000,
            0,
        ));
    }
    let report = run(&calls);
    let f = finding(&report, "premium_subagents").expect("finding");
    assert_eq!(f.count("runs"), 2.0);
    assert_eq!(
        f.count("calls"),
        40.0,
        "main-chat Opus and sub Sonnet are not counted"
    );
    assert_eq!(f.detail["target"], SONNET);
    assert_eq!(f.detail["models"], format!("{FABLE}, {OPUS}"));
    let sonnet = sonnet_price();
    let expected: f64 = calls
        .iter()
        .filter(|c| c.sub && c.model != SONNET)
        .map(|c| {
            let tokens = [10.0, 300.0, 50_000.0, 100_000.0, 0.0];
            let at_target = prices::estimate(&sonnet, &tokens) * 1.4;
            (c.cost.expect("priced") - at_target) * 0.5
        })
        .sum();
    near(f.saving.expect("saving"), expected);
}

#[test]
fn a_cold_resume_is_a_gap_past_the_measured_time_and_a_hot_one_is_not() {
    let main = main_of(SONNET);
    let helper = sub_of("a1", SONNET);
    let t = NOW - 6.0 * HOUR;
    let calls = vec![
        call(&helper, t, 0, 90_000, 0),
        call(&helper, t + 240.0, 100_000, 0, 0),
        call(&helper, t + 240.0 + 700.0, 0, 120_000, 0),
        call(&main, t, 0, 0, 90_000),
        call(&main, t + 40.0 * 60.0, 90_000, 0, 5_000),
        call(&main, t + 40.0 * 60.0 + 4_000.0, 0, 0, 100_000),
    ];
    let report = run(&calls);
    let f = finding(&report, "cold_resumes").expect("finding");
    assert_eq!(
        f.count("sub_rebuilds"),
        1.0,
        "the 11-minute gap on a 5-minute cache"
    );
    assert_eq!(
        f.count("main_rebuilds"),
        1.0,
        "40 minutes is warm on a 1-hour cache; 67 minutes is not"
    );
    let price = sonnet_price();
    let extra_sub = 120_000.0 * (price.input * 1.25 - price.cache_read) / 1e6;
    let extra_main = 100_000.0 * (price.input * 2.0 - price.cache_read) / 1e6;
    near(f.count("sub_extra_usd"), extra_sub);
    near(f.count("main_extra_usd"), extra_main);
}

#[test]
fn a_session_with_no_measured_time_has_no_cold_resumes() {
    let helper = sub_of("a1", SONNET);
    let mut a = call(&helper, NOW - 9_000.0, 0, 90_000, 0);
    a.split = false;
    let mut b = call(&helper, NOW - 100.0, 0, 120_000, 0);
    b.split = false;
    let report = run(&[a, b]);
    assert!(finding(&report, "cold_resumes").is_none());
    assert!(report.unknown.iter().any(|u| u.id == "cache_ttl"));
}

#[test]
fn the_one_hour_helper_cache_is_weighed_from_the_users_own_numbers() {
    let helper = sub_of("a1", SONNET);
    let mut calls = vec![call(&helper, NOW - 9_000.0, 0, 80_000, 0)];
    for n in 1..=3 {
        calls.push(call(
            &helper,
            NOW - 9_000.0 + n as f64 * 900.0,
            0,
            80_000,
            0,
        ));
    }
    let report = run(&calls);
    let f = finding(&report, "cold_resumes").expect("finding");
    assert_eq!(f.count("sub_rebuilds"), 3.0);
    let price = sonnet_price();
    near(
        f.count("ttl1h_added_usd"),
        4.0 * 80_000.0 * price.input * 0.75 / 1e6,
    );
    near(
        f.count("ttl1h_saved_usd"),
        3.0 * 80_000.0 * (price.input * 1.25 - price.cache_read) / 1e6,
    );
    assert_eq!(f.count("ttl1h_pays"), 1.0);
}

#[test]
fn a_model_switch_inside_the_cache_time_is_a_rebuild_of_its_own() {
    let t = NOW - HOUR;
    let calls = vec![
        call(&main_of(SONNET), t, 0, 0, 10_000),
        call(&main_of(SONNET), t + 30.0, 10_000, 0, 0),
        call(&main_of(OPUS), t + 150.0, 0, 0, 120_000),
        call(&main_of(SONNET), t + 200.0, 0, 0, 1_000),
    ];
    let report = run(&calls);
    let f = finding(&report, "model_switches").expect("finding");
    assert_eq!(
        f.count("switches"),
        1.0,
        "the small re-write is not a rebuild"
    );
    assert_eq!(f.count("rewritten_tokens"), 120_000.0);
    assert!(finding(&report, "cold_resumes").is_none());
}

#[test]
fn a_long_run_is_priced_as_fresh_builders_every_hundred_calls() {
    let helper = sub_of("a1", SONNET);
    let calls: Vec<Call> = (0..250)
        .map(|i| {
            call(
                &helper,
                NOW - 20_000.0 + i as f64 * 20.0,
                40_000 + i * 1_000,
                0,
                0,
            )
        })
        .collect();
    let report = run(&calls);
    let f = finding(&report, "long_runs").expect("finding");
    assert_eq!(f.count("runs_over_limit"), 1.0);
    assert_eq!(f.count("max_calls"), 250.0);
    assert!(f.count("calls_over_150k") > 0.0);
    let price = sonnet_price();
    let avoid = 100.0 * 85_000.0 + 50.0 * 185_000.0;
    near(
        f.saving.expect("saving"),
        avoid * price.cache_read / 1e6 * 0.6,
    );
    assert_eq!(f.tokens_avoided, Some((avoid * 0.6) as u64));
}

#[test]
fn repeat_reviews_in_one_session_are_counted_after_the_first() {
    let mut calls = Vec::new();
    for (n, agent) in ["r1", "r2", "r3", "r4"].into_iter().enumerate() {
        let mut spec = sub_of(agent, SONNET);
        spec.agent_type = Some("code-reviewer");
        for i in 0..30 {
            calls.push(call(
                &spec,
                NOW - 50_000.0 + n as f64 * 5_000.0 + i as f64,
                120_000,
                0,
                0,
            ));
        }
    }
    calls.push(call(&sub_of("b1", SONNET), NOW - 40_000.0, 1_000, 0, 0));
    let report = run(&calls);
    let f = finding(&report, "repeated_reviews").expect("finding");
    assert_eq!(f.count("review_runs"), 4.0);
    assert_eq!(f.count("repeat_runs"), 3.0);
    assert_eq!(f.count("most_in_a_session"), 4.0);
    let per_run: f64 = calls[..30].iter().map(|c| c.cost.expect("priced")).sum();
    near(f.saving.expect("saving"), 3.0 * per_run * 0.5);
}

#[test]
fn builders_at_once_over_the_cap_cost_a_share_of_what_runs_alongside() {
    let mut calls = Vec::new();
    let names = ["a1", "a2", "a3", "a4", "a5", "a6", "a7"];
    for name in names {
        for i in 0..10 {
            calls.push(call(
                &sub_of(name, SONNET),
                NOW - 5_000.0 + i as f64,
                100_000,
                0,
                0,
            ));
        }
    }
    calls.push(call(
        &sub_of("late", SONNET),
        NOW - 100_000.0,
        100_000,
        0,
        0,
    ));
    let report = run(&calls);
    let f = finding(&report, "many_builders").expect("finding");
    assert_eq!(f.count("peak_builders"), 7.0);
    assert_eq!(f.count("buckets_over_cap"), 1.0);
    let in_bucket: f64 = calls[..70].iter().map(|c| c.cost.expect("priced")).sum();
    near(f.count("cost_over_cap_usd"), in_bucket);
    near(f.saving.expect("saving"), in_bucket * 0.05);
}

#[test]
fn one_call_is_never_claimed_by_two_findings() {
    let helper = sub_of("a1", OPUS);
    let t = NOW - 30_000.0;
    let mut calls: Vec<Call> = (0..150)
        .map(|i| call(&helper, t + i as f64 * 20.0, 60_000 + i * 2_000, 20_000, 0))
        .collect();
    calls.push(call(&helper, t + 150.0 * 20.0 + 900.0, 0, 200_000, 0));
    let report = run(&calls);
    let total: f64 = calls.iter().map(|c| c.cost.expect("priced")).sum();
    let premium = finding(&report, "premium_subagents").expect("premium");
    assert!(premium.saving.expect("saving") > 0.0);
    let cold = finding(&report, "cold_resumes").expect("rebuild is still evidence");
    assert_eq!(cold.count("sub_rebuilds"), 1.0);
    near(cold.saving.expect("saving"), 0.0);
    let long = finding(&report, "long_runs").expect("long run is still evidence");
    assert_eq!(long.count("runs_over_limit"), 1.0);
    near(long.saving.expect("saving"), 0.0);
    let listed: f64 = report.findings.iter().filter_map(|f| f.saving).sum();
    near(report.saving.expect("total"), listed);
    assert!(
        report.saving.expect("total") <= total,
        "the findings never add up to more than was spent"
    );
}

#[test]
fn overlapping_findings_add_up_to_less_than_the_spend() {
    let mut calls = Vec::new();
    for (n, name) in ["a1", "a2", "a3", "a4", "a5", "a6", "a7"]
        .into_iter()
        .enumerate()
    {
        let mut spec = sub_of(name, if n % 2 == 0 { OPUS } else { SONNET });
        spec.agent_type = Some("reviewer");
        for i in 0..130 {
            calls.push(call(
                &spec,
                NOW - 9_000.0 + i as f64 * 5.0,
                50_000 + i * 1_500,
                0,
                0,
            ));
        }
        calls.push(call(
            &spec,
            NOW - 9_000.0 + 130.0 * 5.0 + 800.0,
            0,
            150_000,
            0,
        ));
    }
    let report = run(&calls);
    let total: f64 = calls.iter().map(|c| c.cost.expect("priced")).sum();
    let all: f64 = report
        .findings
        .iter()
        .chain(&report.not_worth)
        .filter_map(|f| f.saving)
        .sum();
    assert!(all <= total, "{all} vs {total}");
    assert!(report.findings.len() >= 3);
}

#[test]
fn a_model_with_no_price_is_unknown_not_zero() {
    let mut spec = sub_of("a1", "claude-mystery-9");
    spec.agent_type = Some("builder");
    let calls: Vec<Call> = (0..5)
        .map(|i| call(&spec, NOW - 500.0 + i as f64, 1_000, 0, 0))
        .collect();
    let report = run(&calls);
    assert_eq!(report.totals.cost, None);
    assert_eq!(report.totals.unpriced_calls, 5);
    assert_eq!(report.saving, None);
    assert!(report
        .unknown
        .iter()
        .any(|u| u.id == "price" && u.count == 5));
    assert_eq!(report.by_model[0].cost, None);
    assert_eq!(report.by_model[0].cost_share, None);
}

#[test]
fn a_small_lever_goes_under_not_worth_changing_with_its_number() {
    let mut calls = Vec::new();
    for i in 0..10 {
        calls.push(call(
            &main_of(SONNET),
            NOW - 5_000.0 + i as f64,
            900_000,
            0,
            0,
        ));
    }
    calls.push(call(&sub_of("a1", OPUS), NOW - 800.0, 20_000, 0, 0));
    let report = run(&calls);
    assert!(finding(&report, "premium_subagents").is_some());
    assert!(report.findings.iter().all(|f| f.id != "premium_subagents"));
    let small = report
        .not_worth
        .iter()
        .find(|f| f.id == "premium_subagents")
        .expect("listed");
    assert!(small.saving.expect("saving") > 0.0 && small.saving.expect("saving") < 5.0);
}

#[test]
fn the_main_chat_effort_is_not_worth_changing_when_it_is_already_low() {
    let mut spec = main_of(OPUS);
    spec.effort = Some("medium");
    let mut calls: Vec<Call> = (0..10)
        .map(|i| call(&spec, NOW - 500.0 + i as f64, 1_000, 0, 0))
        .collect();
    let report = run(&calls);
    let f = report
        .not_worth
        .iter()
        .find(|f| f.id == "main_effort")
        .expect("listed");
    assert_eq!(f.count("low_or_medium_share"), 1.0);
    for c in &mut calls {
        c.effort = None;
    }
    let blind = run(&calls);
    assert!(blind.not_worth.iter().all(|f| f.id != "main_effort"));
    assert!(blind
        .unknown
        .iter()
        .any(|u| u.id == "effort" && u.count == 10));
}

#[test]
fn money_is_labelled_for_the_plan_and_the_share_is_always_there() {
    let calls: Vec<Call> = (0..30)
        .map(|i| {
            call(
                &sub_of("a1", OPUS),
                NOW - 5_000.0 + i as f64,
                50_000,
                100_000,
                0,
            )
        })
        .collect();
    let mut options = Options::new(NOW);
    options.plan = Plan::Subscription;
    let sub = analyse(&calls, &Catalog::built_in(), &options);
    assert_eq!(sub.money, "api_equivalent");
    assert!(sub.saving_share.expect("share") > 0.0);
    assert!(sub.findings[0].saving_share.is_some());
    options.plan = Plan::Api;
    assert_eq!(
        analyse(&calls, &Catalog::built_in(), &options).money,
        "estimate"
    );
    assert_eq!(run(&calls).money, "api_equivalent");
}

#[test]
fn main_and_helpers_and_models_are_shares_of_the_spend() {
    let calls = vec![
        call(&main_of(OPUS), NOW - 100.0, 100_000, 0, 0),
        call(&sub_of("a1", SONNET), NOW - 90.0, 100_000, 0, 0),
        call(&sub_of("a1", SONNET), NOW - 80.0, 100_000, 0, 0),
    ];
    let report = run(&calls);
    let total: f64 = calls.iter().map(|c| c.cost.expect("priced")).sum();
    near(report.totals.cost.expect("cost"), total);
    let sub = report.by_role.iter().find(|s| s.key == "sub").expect("sub");
    assert_eq!(sub.calls, 2);
    near(
        sub.cost_share.expect("share"),
        (calls[1].cost.expect("c") * 2.0) / total,
    );
    assert_eq!(report.by_model.len(), 2);
    near(
        report.by_model.iter().map(|s| s.token_share).sum::<f64>(),
        1.0,
    );
}

#[test]
fn only_the_asked_days_are_read() {
    let calls = vec![
        call(&main_of(OPUS), NOW - 2.0 * DAY, 1_000, 0, 0),
        call(&main_of(OPUS), NOW - 20.0 * DAY, 1_000, 0, 0),
    ];
    assert_eq!(run(&calls).totals.calls, 1);
    let mut options = Options::new(NOW);
    options.days = 30;
    assert_eq!(
        analyse(&calls, &Catalog::built_in(), &options).totals.calls,
        2
    );
}

#[test]
fn from_transcripts_to_a_ranked_report() {
    use super::ledger_testkit::{line, Use, World, NONE};
    let world = World::new("check-e2e");
    let u = |read, w5, w1| Use {
        input: 10,
        output: 400,
        read,
        w5,
        w1,
        ..NONE
    };
    let mut main = String::new();
    for i in 0..4 {
        main += &line(
            &format!("m{i}"),
            NOW - 60_000.0 + i as f64 * 90.0,
            SONNET,
            u(80_000, 0, if i == 0 { 30_000 } else { 0 }),
            r#","effort":"medium""#,
        );
    }
    world.main("s1", &main);
    let mut idle = String::new();
    for (i, (at, w5)) in [(90_000.0, 30_000), (89_970.0, 0), (87_000.0, 100_000)]
        .into_iter()
        .enumerate()
    {
        idle += &line(&format!("i{i}"), NOW - at, SONNET, u(40_000, w5, 0), "");
    }
    world.main("s2", &idle);
    for agent in ["agent-b1", "agent-b2"] {
        let mut text = String::new();
        for i in 0..6 {
            let at = NOW - 50_000.0 + i as f64 * 25.0 + if i >= 3 { 1_000.0 } else { 0.0 };
            text += &line(
                &format!("{agent}-{i}"),
                at,
                OPUS,
                u(60_000, if i == 3 { 120_000 } else { 20_000 }, 0),
                "",
            );
        }
        world.sub(
            "s1",
            agent,
            r#"{"agentType":"builder","effort":"high"}"#,
            &text,
        );
    }
    for (n, agent) in ["agent-r1", "agent-r2", "agent-r3"].into_iter().enumerate() {
        let mut text = String::new();
        for i in 0..5 {
            text += &line(
                &format!("{agent}-{i}"),
                NOW - 30_000.0 + n as f64 * 2_000.0 + i as f64 * 10.0,
                SONNET,
                u(90_000, 10_000, 0),
                "",
            );
        }
        world.sub("s1", agent, r#"{"agentType":"code-reviewer"}"#, &text);
    }
    let mut ledger = world.ledger();
    assert!(ledger.refresh(NOW, None).complete());
    let calls = ledger.calls_since(NOW - 14.0 * DAY);
    assert_eq!(calls.len(), 4 + 3 + 12 + 15);
    let report = analyse(&calls, &Catalog::built_in(), &Options::new(NOW));
    assert_eq!(report.totals.calls, 34);
    assert_eq!(report.totals.unpriced_calls, 0);
    let ids: Vec<&str> = report.findings.iter().map(|f| f.id).collect();
    assert!(ids.contains(&"premium_subagents"), "{ids:?}");
    assert!(ids.contains(&"repeated_reviews"), "{ids:?}");
    assert!(ids.contains(&"cold_resumes"), "{ids:?}");
    let savings: Vec<f64> = report.findings.iter().filter_map(|f| f.saving).collect();
    assert!(
        savings.windows(2).all(|w| w[0] >= w[1]),
        "ranked: {savings:?}"
    );
    assert_eq!(report.cache.main.one_hour, 1);
    assert_eq!(report.cache.sub.five_min, 5);
    assert_eq!(report.by_role.len(), 2);
    assert!(report.not_worth.iter().any(|f| f.id == "main_effort"));
    let json = serde_json::to_value(&report).expect("json");
    assert_eq!(json["findings"][0]["id"], ids[0]);
}

fn quiet_helpers() -> Vec<Call> {
    let mut calls = Vec::new();
    for (n, agent) in ["a1", "a2"].into_iter().enumerate() {
        for i in 0..8 {
            calls.push(call(
                &sub_of(agent, SONNET),
                NOW - 5_000.0 + n as f64 * 40.0 + i as f64 * 20.0,
                60_000,
                0,
                0,
            ));
        }
    }
    calls
}

#[test]
fn a_finding_that_shows_nothing_wrong_is_not_a_change_to_make() {
    let report = run(&quiet_helpers());
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    let builders = report
        .not_worth
        .iter()
        .find(|f| f.id == "many_builders")
        .expect("measured, so listed");
    assert_eq!(builders.saving, None);
    assert_eq!(builders.count("peak_builders"), 2.0);
    let long = report
        .not_worth
        .iter()
        .find(|f| f.id == "long_runs")
        .expect("measured, so listed");
    assert_eq!(long.count("runs_over_limit"), 0.0);
}

#[test]
fn a_lever_with_nothing_measured_is_left_out_everywhere() {
    let calls = vec![call(&main_of(SONNET), NOW - 100.0, 1_000, 0, 0)];
    let report = run(&calls);
    for id in [
        "many_builders",
        "long_runs",
        "repeated_reviews",
        "premium_subagents",
    ] {
        assert!(finding(&report, id).is_none(), "{id}");
    }
}

#[test]
fn big_contexts_alone_do_not_raise_a_long_run_finding() {
    let calls: Vec<Call> = (0..4)
        .map(|i| {
            call(
                &sub_of("a1", SONNET),
                NOW - 900.0 + i as f64 * 20.0,
                200_000,
                0,
                0,
            )
        })
        .collect();
    let report = run(&calls);
    assert!(report.findings.iter().all(|f| f.id != "long_runs"));
    let long = finding(&report, "long_runs").expect("measured");
    assert!(long.count("calls_over_150k") > 0.0);
}

#[test]
fn a_saving_is_unknown_only_when_a_price_is_missing() {
    let long_run = |model: &'static str| -> Vec<Call> {
        (0..250)
            .map(|i| {
                call(
                    &sub_of("a1", model),
                    NOW - 20_000.0 + i as f64 * 20.0,
                    40_000,
                    0,
                    0,
                )
            })
            .collect()
    };
    let report = run(&long_run(SONNET));
    assert!(report.findings.iter().all(|f| f.saving.is_some()));
    let report = run(&long_run("claude-mystery-9"));
    let long = report
        .findings
        .iter()
        .find(|f| f.id == "long_runs")
        .expect("raised");
    assert_eq!(long.saving, None);
    assert!(report.unknown.iter().any(|u| u.id == "price"));
}
