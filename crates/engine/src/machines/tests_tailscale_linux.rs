use super::catalog;
use super::engine_plan::AdminAnswer;
use super::fakes_b::*;
use super::registry::Machine;
use super::runner::{Channel, CommandResult};
use super::step::{CheckResult, FixContext, FixOutcome, RemoteOutput, SetupStep, StepFix, StepId};
use super::steps_node;
use super::steps_tailscale_linux::{is_sign_in_link, with_fix, INSTALL, SIGN_IN};
use super::tests_engine_world::{lock, sid, Setup, StepBuilder, World};
use super::tests_node_steps::{node_linux, node_mac};
use crate::machines::step::TailscaleState;
use std::sync::{Arc, Mutex};

const LINK: &str = "https://login.tailscale.com/a/abc123DEF456ghi";
const SECRET_PART: &str = "abc123DEF456ghi";
const LOGGED_OUT: &str = r#"{"BackendState":"NeedsLogin","Self":{"TailscaleIPs":[]}}"#;
const RUNNING: &str = r#"{"BackendState":"Running","Self":{"TailscaleIPs":["100.101.102.103"]}}"#;
const PROBE: &str = "tailscale status --json --peers=false";

fn check(m: &Machine, text: &str, code: i32) -> CheckResult {
    let rig = Rig::new(m.clone());
    let list = catalog::steps(m, std::slice::from_ref(m));
    let (_, parse) = remote(&find(&list, &StepId::NODE_TAILSCALE));
    parse(
        &RemoteOutput {
            text: text.into(),
            code,
        },
        &rig.step(),
    )
}

fn tailscale_step(m: &Machine, state: Option<TailscaleState>) -> SetupStep {
    let list = catalog::steps(m, std::slice::from_ref(m));
    find(&with_fix(list, m, state), &StepId::NODE_TAILSCALE)
}

struct Seen {
    log: Arc<Mutex<String>>,
    offered: Arc<Mutex<Vec<String>>>,
}

fn watched(rig: &Rig) -> (FixContext, Seen) {
    let mut ctx = rig.fix();
    let log = Arc::new(Mutex::new(String::new()));
    let offered = Arc::new(Mutex::new(Vec::<String>::new()));
    let (l, o) = (log.clone(), offered.clone());
    ctx.log = Arc::new(move |_, t: &str| l.lock().unwrap().push_str(t));
    ctx.links = Some(Arc::new(move |u: &str| {
        o.lock().unwrap().push(u.to_string())
    }));
    (ctx, Seen { log, offered })
}

fn sign_in_call(a: &[String]) -> bool {
    a.last().is_some_and(|s| s.contains("tailscale up"))
}

#[test]
fn missing_tailscale_offers_install() {
    let m = node_linux();
    let seen = check(&m, "sh: tailscale: not found", 127);
    assert!(seen.state.is_fail());
    assert_eq!(seen.detected.tailscale, Some(TailscaleState::Missing));

    let step = tailscale_step(&m, Some(TailscaleState::Missing));
    let plan = step.plan().expect("Install is a Fix").clone();
    assert_eq!(
        plan.display,
        "curl -fsSL https://tailscale.com/install.sh -o /tmp/ts-install.sh && sh /tmp/ts-install.sh; rm -f /tmp/ts-install.sh"
    );
    assert_eq!(plan.display, INSTALL);
    assert!(plan.needs_admin);

    let rig = Rig::new(m);
    let (ctx, _) = watched(&rig);
    let out = (plan.run)(&ctx);
    assert!(out.ok, "{}", out.message);
    let calls = rig.calls();
    assert!(
        calls[0].argv.iter().any(|a| a == "root@w1.test"),
        "{:?}",
        calls[0].argv
    );
    assert!(unquoted(last(&calls[0].argv)).contains(INSTALL));
    assert!(calls[0].stdin.is_none());
    assert!(!calls[0].argv.join(" ").contains("sudo"));
}

#[test]
fn install_that_leaves_no_tailscale_fails() {
    let rig = Rig::new(node_linux());
    rig.runner.on(
        |a| a.last().is_some_and(|s| s.contains("command -v tailscale")),
        CommandResult::new(1, "", ""),
    );
    let (ctx, _) = watched(&rig);
    let plan = tailscale_step(&rig.machine, Some(TailscaleState::Missing))
        .plan()
        .unwrap()
        .clone();
    let out = (plan.run)(&ctx);
    assert!(!out.ok);
    assert!(out.message.contains("Tailscale"), "{}", out.message);
}

#[test]
fn logged_out_offers_sign_in() {
    let m = node_linux();
    let seen = check(&m, LOGGED_OUT, 0);
    assert!(seen.state.is_fail());
    assert_eq!(seen.detected.tailscale, Some(TailscaleState::LoggedOut));

    let step = tailscale_step(&m, Some(TailscaleState::LoggedOut));
    let plan = step.plan().expect("Sign In is a Fix").clone();
    assert_eq!(plan.display, "tailscale up --timeout=300s");
    assert_eq!(plan.display, SIGN_IN);
    assert!(plan.needs_admin);

    let rig = Rig::new(m);
    rig.runner.on_with(
        sign_in_call,
        CommandResult::ok(""),
        vec![(
            Channel::Stderr,
            format!("To authenticate, visit:\n\n\t{LINK}\n"),
        )],
    );
    let (ctx, seen) = watched(&rig);
    let out = (plan.run)(&ctx);
    assert!(out.ok, "{}", out.message);
    let calls = rig.calls();
    assert!(calls[0].argv.iter().any(|a| a == "root@w1.test"));
    assert!(unquoted(last(&calls[0].argv)).contains(SIGN_IN));
    assert_eq!(seen.offered.lock().unwrap().clone(), vec![LINK.to_string()]);
}

#[test]
fn states_other_than_missing_or_logged_out_keep_the_howto() {
    let m = node_linux();
    for (text, code) in [
        (RUNNING, 0),
        (
            r#"{"BackendState":"Stopped","Self":{"TailscaleIPs":[]}}"#,
            0,
        ),
        (r#"{"BackendState":"NeedsLogin\n; rm -rf /"}"#, 0),
        ("ssh: connect to host w1 port 22: refused", 255),
        ("", 1),
    ] {
        assert_eq!(check(&m, text, code).detected.tailscale, None, "{text}");
    }
    assert!(matches!(tailscale_step(&m, None).fix, StepFix::HowTo(_)));
}

#[test]
fn mac_stays_howto() {
    let m = node_mac();
    assert_eq!(check(&m, "not found", 127).detected.tailscale, None);
    assert_eq!(check(&m, LOGGED_OUT, 0).detected.tailscale, None);
    for state in [
        Some(TailscaleState::Missing),
        Some(TailscaleState::LoggedOut),
    ] {
        assert!(
            matches!(tailscale_step(&m, state).fix, StepFix::HowTo(_)),
            "{state:?}"
        );
    }
}

#[test]
fn sign_in_link_never_in_log() {
    let rig = Rig::new(node_linux());
    let (first, second) = LINK.split_at(40);
    rig.runner.on_with(
        sign_in_call,
        CommandResult::new(
            1,
            &format!("out {LINK}\n"),
            &format!("failed after {LINK}\n"),
        ),
        vec![
            (
                Channel::Stderr,
                format!("To authenticate, visit:\n\n\t{first}"),
            ),
            (Channel::Stderr, format!("{second}\nWaiting for you\n")),
            (
                Channel::Stdout,
                format!("again {LINK} and more\ntail {first}"),
            ),
        ],
    );
    let (ctx, seen) = watched(&rig);
    let plan = tailscale_step(&rig.machine, Some(TailscaleState::LoggedOut))
        .plan()
        .unwrap()
        .clone();
    let out = (plan.run)(&ctx);
    assert!(!out.ok);
    let log = seen.log.lock().unwrap().clone();
    for text in [&log, &out.message, &format!("{out:?}")] {
        assert!(!text.contains(SECRET_PART), "{text}");
        assert!(!text.contains("login.tailscale.com"), "{text}");
    }
    assert!(log.contains("To authenticate, visit:"), "{log}");
    assert!(log.contains("Waiting for you"), "{log}");
    assert_eq!(seen.offered.lock().unwrap().clone(), vec![LINK.to_string()]);
    for call in rig.calls() {
        assert!(!call.argv.join(" ").contains(SECRET_PART));
    }
}

#[test]
fn a_link_with_another_host_or_odd_characters_is_not_offered() {
    let rig = Rig::new(node_linux());
    let bad = [
        "https://login.tailscale.com.evil.example/a/abc",
        "https://evil.example/login.tailscale.com/a/abc",
        "http://login.tailscale.com/a/abc",
        "https://login.tailscale.com@evil.example/a/abc",
        "https://login.tailscale.com:8443/a/abc",
        "https://login.tailscale.com/a/abc<script>",
        "https://login.tailscale.com/a/ab\u{202e}c",
        "https://login.tailscale.com/",
        "https://login.tailscale.com",
    ];
    let lines: String = bad.iter().map(|b| format!("visit {b}\n")).collect();
    rig.runner.on_with(
        sign_in_call,
        CommandResult::ok(""),
        vec![(Channel::Stderr, lines)],
    );
    let (ctx, seen) = watched(&rig);
    let plan = tailscale_step(&rig.machine, Some(TailscaleState::LoggedOut))
        .plan()
        .unwrap()
        .clone();
    assert!((plan.run)(&ctx).ok);
    assert!(seen.offered.lock().unwrap().is_empty());
    assert!(!seen.log.lock().unwrap().contains("login.tailscale.com"));
    for url in bad {
        assert!(!is_sign_in_link(url), "{url}");
    }
    assert!(is_sign_in_link(LINK));
    assert!(is_sign_in_link(
        "https://login.tailscale.com/a/0123456789ab"
    ));
    assert!(!is_sign_in_link(&format!(
        "https://login.tailscale.com/a/{}",
        "x".repeat(400)
    )));
    assert!(!is_sign_in_link(&format!("{LINK}\n")));
    assert!(!is_sign_in_link(&format!("{LINK} ")));
}

fn world(machine: Machine, audit: &std::path::Path) -> Arc<World> {
    let mut s = Setup::new(|m, _| {
        let mut list = vec![StepBuilder::new("reachable").build()];
        list.extend(steps_node::steps(m));
        list
    });
    s.machines = vec![machine];
    s.audit = Some(audit.to_path_buf());
    Arc::new(s.build())
}

fn audit_path(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-ts-linux-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("audit.jsonl")
}

fn tailscale_display(w: &World) -> Option<String> {
    let m = lock(&w.store.list)[0].clone();
    w.engine
        .steps(&m)
        .into_iter()
        .find(|s| s.id == StepId::NODE_TAILSCALE)
        .and_then(|s| s.plan().map(|p| p.display.clone()))
}

#[test]
fn the_engine_picks_the_fix_from_the_probe_result() {
    let audit = audit_path("pick");
    let w = world(node_linux(), &audit);
    assert_eq!(tailscale_display(&w), None);
    w.answer(PROBE, "sh: tailscale: not found", 127);
    w.refresh("w1", false);
    assert_eq!(tailscale_display(&w).as_deref(), Some(INSTALL));
    w.answer(PROBE, LOGGED_OUT, 0);
    w.refresh("w1", false);
    assert_eq!(tailscale_display(&w).as_deref(), Some(SIGN_IN));
    w.answer(PROBE, RUNNING, 0);
    w.refresh("w1", false);
    assert_eq!(tailscale_display(&w), None);
    let _ = std::fs::remove_dir_all(audit.parent().unwrap());
}

#[test]
fn engine_keeps_the_link_out_of_log_audit_and_outcome_and_hands_it_over() {
    let audit = audit_path("link");
    let w = world(node_linux(), &audit);
    w.runner.on_with(
        sign_in_call,
        CommandResult::ok(""),
        vec![(
            Channel::Stderr,
            format!("To authenticate, visit:\n\n\t{LINK}\n"),
        )],
    );
    w.answer(PROBE, LOGGED_OUT, 0);
    w.refresh("w1", false);
    let seen: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let (s, w2) = (seen.clone(), w.clone());
    w.engine.set_on_link(Some(Arc::new(move || {
        lock(&s).push(w2.engine.sign_in_link("w1"))
    })));
    assert_eq!(w.engine.sign_in_link("w1"), None);
    let out: FixOutcome = w.fix("w1", "node-tailscale", AdminAnswer::RootConfirmed);
    assert!(out.ok, "{}", out.message);
    assert_eq!(lock(&seen).clone(), vec![Some(LINK.to_string())]);
    assert_eq!(
        w.engine.sign_in_link("w1"),
        None,
        "the link ends with the Fix"
    );
    let log = w.log("w1");
    assert!(log.contains("To authenticate"), "{log}");
    let audit_text = std::fs::read_to_string(&audit).unwrap_or_default();
    assert!(audit_text.contains("node-tailscale") || audit_text.contains("Tailscale"));
    for text in [&log, &out.message, &audit_text] {
        assert!(!text.contains(SECRET_PART), "{text}");
        assert!(!text.contains("login.tailscale.com"), "{text}");
    }
    let _ = (sid("x"), std::fs::remove_dir_all(audit.parent().unwrap()));
}
