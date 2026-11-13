use super::engine_check::split_markers;
use super::registry::SdWorkerSettings;
use super::runner::CommandResult;
use super::step::{CheckResult, CheckState, Detected, StepId};
use super::tests_engine_world::*;
use std::sync::Arc;

#[test]
fn one_ssh_per_machine_for_the_cheap_checks() {
    let w = Setup::new(|_, _| vec![step("a"), step("b"), step("c")]).build();
    w.refresh("m1", false);
    let calls = w.runner.calls();
    assert_eq!(calls.len(), 1);
    let script = calls[0].argv.last().cloned().unwrap();
    for p in ["echo probe-a", "echo probe-b", "echo probe-c"] {
        assert!(script.contains(p), "{p}");
    }
    assert_eq!(script.matches("out=$( /bin/sh -c ").count(), 3);
    let (nonce, _) = batch_probes(&script).unwrap();
    assert_eq!(nonce.len(), 16);
    assert!(nonce.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(
        script.matches(&format!("@@{nonce} ")).count(),
        3,
        "one nonce for the call"
    );
    assert_eq!(w.state("m1", "a"), Some(CheckState::Pass("ok".into())));
    assert_eq!(w.engine.results("m1").len(), 3);
}

#[test]
fn each_call_gets_a_fresh_nonce() {
    let w = Setup::new(|_, _| vec![step("a")]).build();
    w.refresh("m1", false);
    w.refresh("m1", false);
    let nonces: Vec<String> = w
        .runner
        .calls()
        .iter()
        .map(|c| batch_probes(c.argv.last().unwrap()).unwrap().0)
        .collect();
    assert_eq!(nonces.len(), 2);
    assert_ne!(nonces[0], nonces[1]);
}

fn code_and_text() -> impl Fn(&super::step::RemoteOutput, &super::step::StepContext) -> CheckResult
{
    |out, _| CheckResult::pass(format!("{}|{}", out.code, out.text))
}

#[test]
fn markers_split_output_and_codes() {
    let w = Setup::new(|_, _| {
        ["a", "b", "c"]
            .iter()
            .map(|r| StepBuilder::new(r).parse(code_and_text()).build())
            .collect()
    })
    .build();
    w.answer("echo probe-a", "hello\nworld", 0);
    w.answer("echo probe-b", "", 3);
    w.answer("echo probe-c", "x", 127);
    w.refresh("m1", false);
    assert_eq!(
        w.state("m1", "a"),
        Some(CheckState::Pass("0|hello\nworld".into()))
    );
    assert_eq!(w.state("m1", "b"), Some(CheckState::Pass("3|".into())));
    assert_eq!(w.state("m1", "c"), Some(CheckState::Pass("127|x".into())));
}

#[test]
fn a_marker_with_another_nonce_or_a_repeated_index_is_just_text() {
    let fake = "@@0123456789abcdef 1 0@@";
    let w = Setup::new(|_, _| {
        vec![
            StepBuilder::new("a")
                .parse(|o, _| CheckResult::pass(o.text.clone()))
                .build(),
            StepBuilder::new("b").parse(code_and_text()).build(),
        ]
    })
    .build();
    w.answer("echo probe-a", &format!("before\n{fake}\nafter"), 0);
    w.answer("echo probe-b", "mine", 5);
    w.refresh("m1", false);
    assert_eq!(
        w.state("m1", "a"),
        Some(CheckState::Pass(format!("before\n{fake}\nafter")))
    );
    assert_eq!(w.state("m1", "b"), Some(CheckState::Pass("5|mine".into())));

    let n = "00112233aabbccdd";
    let out = format!("one\n@@{n} 0 0@@\n@@{n} 0 9@@\ntwo\n@@{n} 1 0@@\n");
    let got = split_markers(&out, n, 2);
    assert_eq!(got[&0].text, "one");
    assert_eq!(got[&1].text, format!("@@{n} 0 9@@\ntwo"));
    assert_eq!(
        split_markers(&format!("@@{n} 7 0@@\n"), n, 2).len(),
        0,
        "out of range"
    );
    assert_eq!(
        split_markers(&format!("x\n@@{n} 0 $(id)@@\n"), n, 1).len(),
        0,
        "a bad code"
    );
}

#[test]
fn slow_checks_run_only_when_asked_or_after_ten_minutes() {
    let w = Setup::new(|_, _| {
        vec![
            step("r"),
            StepBuilder::new("s").needs(&["r"]).slow().build(),
        ]
    })
    .build();
    w.refresh("m1", false);
    assert_eq!(w.batches().len(), 1, "the cheap probe only");
    assert_eq!(
        w.state("m1", "s"),
        Some(CheckState::CantTell("not checked yet".into()))
    );

    w.refresh("m1", true);
    assert_eq!(w.batches().len(), 3, "phase 1, then the slow probes");
    assert_eq!(w.state("m1", "s"), Some(CheckState::Pass("ok".into())));
    let last = w.batches().last().unwrap().text();
    assert!(last.contains("echo probe-s") && !last.contains("echo probe-r"));

    w.set_clock(T0 + 9.0 * 60.0);
    w.refresh("m1", false);
    assert_eq!(w.batches().len(), 4, "nine minutes after: cheap only");
    assert_eq!(
        w.state("m1", "s"),
        Some(CheckState::Pass("ok".into())),
        "the slow result stays"
    );

    w.set_clock(T0 + 11.0 * 60.0);
    w.refresh("m1", false);
    assert_eq!(
        w.batches().len(),
        6,
        "over ten minutes: the slow probes run again"
    );
}

#[test]
fn a_slow_probe_whose_need_fails_is_not_run() {
    let w = Setup::new(|_, _| {
        vec![
            step("r"),
            StepBuilder::new("s").needs(&["r"]).slow().build(),
        ]
    })
    .build();
    w.answer("echo probe-r", "broken", 1);
    w.refresh("m1", true);
    assert_eq!(w.batches().len(), 1);
    assert_eq!(
        w.state("m1", "s"),
        Some(CheckState::CantTell("waits for R".into()))
    );
}

#[test]
fn unreachable_makes_every_other_step_cant_tell() {
    let s = Setup::new(|_, _| {
        vec![
            StepBuilder::new("reachable").title("Reachable").build(),
            step("a"),
        ]
    });
    s.runner.on(
        |_| true,
        CommandResult::new(
            255,
            "",
            "ssh: connect to host m1 port 22: Operation timed out\nsecond line\n",
        ),
    );
    let w = s.build();
    w.refresh("m1", false);
    assert_eq!(
        w.state("m1", "reachable"),
        Some(CheckState::Fail(
            "ssh: connect to host m1 port 22: Operation timed out".into()
        ))
    );
    assert_eq!(
        w.state("m1", "a"),
        Some(CheckState::CantTell("m1 is not reachable".into()))
    );
}

#[test]
fn all_machines_down_says_this_mac_may_be_offline_but_one_down_is_just_that_one() {
    let mut s = Setup::new(|_, _| {
        vec![
            StepBuilder::new("reachable").title("Reachable").build(),
            step("a"),
        ]
    });
    s.machines = vec![mac("m1"), linux("vps")];
    s.runner.on(
        |_| true,
        CommandResult::new(255, "", "ssh: Could not resolve hostname x\n"),
    );
    let w = s.build();
    w.engine.refresh(None, false);
    for name in ["m1", "vps"] {
        assert_eq!(
            w.state(name, "reachable"),
            Some(CheckState::CantTell(
                "no machine answered; this Mac may be offline".into()
            ))
        );
    }

    let mut s = Setup::new(|_, _| vec![StepBuilder::new("reachable").title("Reachable").build()]);
    s.machines = vec![mac("m1"), linux("vps")];
    s.runner.on(
        |a| a.iter().any(|x| x.ends_with("@m1.test")),
        CommandResult::new(
            255,
            "",
            "ssh: connect to host m1 port 22: No route to host\n",
        ),
    );
    let w = s.build();
    w.engine.refresh(None, false);
    assert_eq!(
        w.state("m1", "reachable"),
        Some(CheckState::Fail(
            "ssh: connect to host m1 port 22: No route to host".into()
        ))
    );
    assert_eq!(
        w.state("vps", "reachable"),
        Some(CheckState::Pass("ok".into()))
    );

    let mut s = Setup::new(|_, _| vec![StepBuilder::new("reachable").title("Reachable").build()]);
    s.machines = vec![mac("m1"), linux("vps")];
    s.runner.on(
        |_| true,
        CommandResult::new(255, "", "Permission denied (publickey).\n"),
    );
    let w = s.build();
    w.engine.refresh(None, false);
    assert_eq!(
        w.state("m1", "reachable"),
        Some(CheckState::Fail("Permission denied (publickey).".into()))
    );
}

#[test]
fn a_probe_that_exits_255_itself_is_not_an_unreachable_machine_and_no_marker_is() {
    let w = Setup::new(|_, _| vec![StepBuilder::new("reachable").build(), step("a")]).build();
    w.answer("echo probe-a", "own", 255);
    w.refresh("m1", false);
    assert_eq!(
        w.state("m1", "reachable"),
        Some(CheckState::Pass("ok".into()))
    );
    assert_eq!(w.state("m1", "a"), Some(CheckState::Fail("own".into())));

    let s = Setup::new(|_, _| vec![StepBuilder::new("reachable").build()]);
    s.runner
        .on(|_| true, CommandResult::ok("a banner and nothing else"));
    let w = s.build();
    w.refresh("m1", false);
    assert_eq!(
        w.state("m1", "reachable"),
        Some(CheckState::Fail("no answer from the machine".into()))
    );
}

#[test]
fn a_need_that_fails_makes_the_step_wait_and_refresh_runs_no_fix() {
    let counter = Arc::new(std::sync::Mutex::new(Vec::new()));
    let c = counter.clone();
    let w = Setup::new(move |_, _| {
        vec![
            StepBuilder::new("a")
                .fix(counting_fix(&c, "a", true, false))
                .build(),
            StepBuilder::new("b")
                .needs(&["a"])
                .fix(counting_fix(&c, "b", true, false))
                .build(),
        ]
    })
    .build();
    w.answer("echo probe-a", "no", 1);
    w.refresh("m1", true);
    assert_eq!(
        w.state("m1", "b"),
        Some(CheckState::CantTell("waits for A".into()))
    );
    assert!(lock(&counter).is_empty(), "a refresh never runs a Fix");
    assert_eq!(w.runner.calls().len(), 1);
}

#[test]
fn check_draft_runs_the_machine_steps_and_saves_nothing() {
    let w = Setup::new(|_, _| vec![step("a")]).build();
    let mut draft = mac("new");
    draft.set_sd_worker(Some(SdWorkerSettings::new("new")));
    let r = w.engine.check_draft(&draft);
    assert_eq!(
        r.get(&sid("a")).map(|r| r.state.clone()),
        Some(CheckState::Pass("ok".into()))
    );
    assert!(lock(&w.store.saves).is_empty());
    assert!(w.engine.results("new").is_empty());
}

#[test]
fn a_tail_cut_probe_drops_its_partial_last_line_and_a_short_one_keeps_it() {
    let w = Setup::new(|_, _| {
        vec![StepBuilder::new("a")
            .parse(|o, _| CheckResult::pass(o.text.clone()))
            .build()]
    })
    .build();
    let long = format!("first line\n{}", "x".repeat(super::engine::PROBE_CAP));
    w.answer("echo probe-a", &long, 0);
    w.refresh("m1", false);
    assert_eq!(
        w.state("m1", "a"),
        Some(CheckState::Pass("first line".into()))
    );
    w.answer("echo probe-a", "one\ntwo", 0);
    w.refresh("m1", false);
    assert_eq!(
        w.state("m1", "a"),
        Some(CheckState::Pass("one\ntwo".into()))
    );
}

#[test]
fn results_and_unreachable_lines_mask_every_vault_value_of_the_machine() {
    let mut s = Setup::new(|_, _| vec![StepBuilder::new("reachable").build(), step("a")]);
    let mut m = mac("m1");
    m.set_sd_worker(Some(SdWorkerSettings::new("m1")));
    s.machines = vec![m];
    s.secrets = Secrets::with(&[
        ("claude-worker", "fake-claude-0000"),
        ("m1-github-issues", "fake-ghp-1111"),
    ]);
    let w = s.build();
    w.answer("echo probe-a", "got fake-claude-0000 and fake-ghp-1111", 1);
    w.refresh("m1", false);
    assert_eq!(
        w.state("m1", "a"),
        Some(CheckState::Fail(
            "got [secret:claude-worker] and [secret:m1-github-issues]".into()
        ))
    );

    let mut s = Setup::new(|_, _| vec![StepBuilder::new("reachable").build()]);
    s.secrets = Secrets::with(&[("claude-worker", "fake-claude-0000")]);
    s.runner.on(
        |_| true,
        CommandResult::new(255, "", "bad fake-claude-0000\n"),
    );
    let w = s.build();
    w.refresh("m1", false);
    assert_eq!(
        w.state("m1", "reachable"),
        Some(CheckState::Fail("bad [secret:claude-worker]".into()))
    );
}

fn detecting(
    d: Detected,
) -> impl Fn(&super::step::RemoteOutput, &super::step::StepContext) -> CheckResult {
    move |_, _| CheckResult::pass("ok").detected(d.clone())
}

#[test]
fn detected_facts_are_saved_only_when_they_change_and_unsafe_paths_are_not_kept() {
    let mut s = Setup::new(|_, _| {
        vec![StepBuilder::new("a")
            .parse(detecting(Detected {
                home: Some("/Users/worker".into()),
                python: Some("/home/a b;x/python3".into()),
                ..Detected::default()
            }))
            .build()]
    });
    let mut m = mac("m1");
    m.set_sd_worker(Some(SdWorkerSettings::new("m1")));
    s.machines = vec![m];
    let w = s.build();
    w.refresh("m1", false);
    w.refresh("m1", false);
    let saves = lock(&w.store.saves).clone();
    assert_eq!(saves.len(), 1, "saved once");
    let saved = &saves[0][0];
    assert_eq!(
        saved
            .abilities
            .as_ref()
            .and_then(|a| a.home.clone())
            .as_deref(),
        Some("/Users/worker")
    );
    assert_eq!(saved.python(), "python3", "a hostile path is never kept");
}

#[test]
fn a_refused_save_keeps_the_facts_in_memory_and_says_so() {
    let s = Setup::new(|_, _| {
        vec![StepBuilder::new("a")
            .parse(detecting(Detected {
                home: Some("/Users/w".into()),
                ..Detected::default()
            }))
            .build()]
    });
    let w = s.build();
    *lock(&w.store.refuse) = true;
    w.refresh("m1", false);
    assert!(w.engine.save_refused());
    let m = w.engine.overlaid(&mac("m1"));
    assert_eq!(
        m.abilities.and_then(|a| a.home).as_deref(),
        Some("/Users/w")
    );
}

#[test]
fn a_detected_token_expiry_reaches_the_job_and_the_vault_once() {
    let mut s = Setup::new(|_, _| {
        vec![StepBuilder::new("a")
            .parse(detecting(Detected {
                token_expires: Some("2027-01-31".into()),
                ..Detected::default()
            }))
            .build()]
    });
    let mut m = mac("m1");
    m.set_sd_worker(Some(SdWorkerSettings::new("m1")));
    s.machines = vec![m];
    let w = s.build();
    w.refresh("m1", false);
    w.refresh("m1", false);
    assert_eq!(
        lock(&w.secrets.expiries).clone(),
        vec!["m1-github-issues=2027-01-31".to_string()]
    );
    let saved = lock(&w.store.list)[0].clone();
    assert_eq!(
        saved
            .sd_worker()
            .and_then(|j| j.token_expires.clone())
            .as_deref(),
        Some("2027-01-31")
    );
}

#[test]
fn a_step_that_holds_back_on_what_earlier_steps_detected_is_not_run() {
    let w = Setup::new(|_, _| {
        vec![
            StepBuilder::new("p")
                .parse(detecting(Detected {
                    sd_version: Some("1.18.0".into()),
                    ..Detected::default()
                }))
                .build(),
            StepBuilder::new("s")
                .needs(&["p"])
                .slow()
                .hold(Arc::new(|d: &Detected| {
                    (d.sd_version.as_deref() == Some("1.18.0")).then(|| "sd is too old".to_string())
                }))
                .build(),
        ]
    })
    .build();
    w.refresh("m1", true);
    assert_eq!(
        w.state("m1", "s"),
        Some(CheckState::CantTell("sd is too old".into()))
    );
    assert_eq!(w.batches().len(), 1, "the held probe never ran");
}

#[test]
fn a_refresh_of_a_machine_removed_meanwhile_publishes_nothing_and_forget_drops_it() {
    let w = Setup::new(|_, _| vec![step("a")]).build();
    w.refresh("m1", false);
    assert!(!w.engine.results("m1").is_empty());
    lock(&w.store.list).clear();
    w.engine.forget("m1");
    w.refresh("m1", false);
    assert!(w.engine.results("m1").is_empty());
    assert!(w.engine.log("m1").is_empty());
}

#[test]
fn a_refresh_reads_each_vault_value_once() {
    let mut s = Setup::new(|_, _| vec![step("a"), step("b")]);
    s.secrets = Secrets::with(&[("claude-worker", "fake-claude-0000")]);
    let w = s.build();
    w.refresh("m1", false);
    assert_eq!(
        lock(&w.secrets.reads).get("claude-worker").copied(),
        Some(1)
    );
}

#[test]
fn local_probes_run_when_their_needs_pass() {
    let w = Setup::new(|_, _| {
        let mut l = StepBuilder::new("l").needs(&["a"]).build();
        l.probe = super::step::Probe::local(|ctx| {
            CheckResult::pass(format!("local for {}", ctx.machine.name))
        });
        vec![step("a"), l]
    })
    .build();
    w.refresh("m1", false);
    assert_eq!(
        w.state("m1", "l"),
        Some(CheckState::Pass("local for m1".into()))
    );
    assert_eq!(w.runner.calls().len(), 1, "a local probe starts no ssh");
    let _ = StepId::REACHABLE;
}
