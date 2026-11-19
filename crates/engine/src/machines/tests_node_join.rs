use super::fakes_b::*;
use super::registry::{Machine, NodeChoices, NodeSettings};
use super::runner::{Channel, CommandResult, ScriptedRunner};
use super::step::{CheckResult, ClusterTokens, FixContext, Probe, RemoteOutput, StepId};
use super::tests_node_steps::{node_linux, step};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const TOKEN: &str = "nlj1.PLANTEDINVITETOKENVALUE0123456789";
const OTHER_TOKEN: &str = "nlj1.SECONDINVITETOKENVALUE0123456789ab";
const ME: &str = "0123456789abcdef";
const STALE: &str = "fedcba9876543210";
const CLUSTER: &str = "cluster-aaaa";
const NOT_APP: &str = "Add Machine";

type Invite = (String, String, bool, bool);

struct Fake {
    tailnet: Option<String>,
    primary: Option<(String, u16)>,
    invites: Mutex<Vec<Invite>>,
    answers: Mutex<VecDeque<Result<Option<String>, String>>>,
    token: Result<String, String>,
    runner: Option<Arc<ScriptedRunner>>,
    seen: AtomicUsize,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            tailnet: None,
            primary: None,
            invites: Mutex::new(Vec::new()),
            answers: Mutex::new(VecDeque::new()),
            token: Err("forbidden".into()),
            runner: None,
            seen: AtomicUsize::new(usize::MAX),
        }
    }
    fn primary(mut self, addr: &str, port: u16) -> Fake {
        self.primary = Some((addr.into(), port));
        self
    }
    fn answer(self, a: Result<Option<String>, String>) -> Fake {
        self.answers.lock().unwrap().push_back(a);
        self
    }
    fn token(mut self, t: Result<String, String>) -> Fake {
        self.token = t;
        self
    }
    fn watching(mut self, runner: &Arc<ScriptedRunner>) -> Fake {
        self.runner = Some(runner.clone());
        self
    }
    fn invites(&self) -> Vec<Invite> {
        self.invites.lock().unwrap().clone()
    }
}

impl ClusterTokens for Fake {
    fn join_token(&self) -> Result<String, String> {
        self.token.clone()
    }
    fn cluster_id(&self) -> Result<Option<String>, String> {
        Ok(Some(CLUSTER.into()))
    }
    fn can_be_master(&self, _machine: &str) -> Option<bool> {
        None
    }
    fn invite(
        &self,
        node: &str,
        name: &str,
        can_be_master: bool,
        acting: bool,
    ) -> Result<Option<String>, String> {
        if let Some(r) = &self.runner {
            self.seen.store(r.calls().len(), Ordering::SeqCst);
        }
        self.invites
            .lock()
            .unwrap()
            .push((node.into(), name.into(), can_be_master, acting));
        self.answers.lock().unwrap().pop_front().unwrap_or(Ok(None))
    }
    fn primary_addr(&self) -> Option<(String, u16)> {
        self.primary.clone()
    }
    fn tailnet(&self) -> Option<String> {
        self.tailnet.clone()
    }
}

fn tailscale_json(tailnet: Option<&str>) -> String {
    let net = tailnet
        .map(|n| format!(r#","CurrentTailnet":{{"Name":"{n}"}}"#))
        .unwrap_or_default();
    format!(r#"{{"BackendState":"Running","Self":{{"TailscaleIPs":["100.101.102.103"]}}{net}}}"#)
}

fn tailscale_check(machine_net: Option<&str>, local: Option<&str>) -> CheckResult {
    let rig = Rig::new(node_linux());
    let (_, parse) = remote(&step(&rig.machine, &StepId::NODE_TAILSCALE));
    let mut ctx = rig.step();
    let mut fake = Fake::new();
    fake.tailnet = local.map(str::to_string);
    ctx.tokens = Arc::new(fake);
    parse(
        &RemoteOutput {
            text: tailscale_json(machine_net),
            code: 0,
        },
        &ctx,
    )
}

#[test]
fn same_tailnet_passes() {
    let r = tailscale_check(Some("example.ts.net"), Some("example.ts.net"));
    assert!(r.state.is_pass(), "{:?}", r.state);
}

#[test]
fn other_tailnet_fails_with_both_names() {
    let r = tailscale_check(Some("other.ts.net"), Some("example.ts.net"));
    assert!(r.state.is_fail(), "{:?}", r.state);
    assert!(r.state.text().contains("other.ts.net"), "{:?}", r.state);
    assert!(r.state.text().contains("example.ts.net"), "{:?}", r.state);
}

#[test]
fn unknown_local_tailnet_keeps_todays_pass() {
    let r = tailscale_check(Some("other.ts.net"), None);
    assert!(r.state.is_pass(), "{:?}", r.state);
    let r = tailscale_check(None, Some("example.ts.net"));
    assert!(r.state.is_pass(), "{:?}", r.state);
}

fn reach_check(rig: &Rig, fake: Fake) -> CheckResult {
    let s = step(&rig.machine, &StepId::NODE_REACH);
    let Probe::Local(p) = &s.probe else {
        panic!("Reaches the Primary runs from this Mac");
    };
    let mut ctx = rig.step();
    ctx.tokens = Arc::new(fake);
    p(&ctx)
}

#[test]
fn reach_passes_on_exit_zero() {
    let rig = Rig::new(node_linux());
    rig.runner.on_script("cluster reach", ok(""));
    let r = reach_check(&rig, Fake::new().primary("100.101.102.7", 47823));
    assert!(r.state.is_pass(), "{:?}", r.state);
    let calls = rig.calls();
    assert_eq!(calls.len(), 1);
    assert!(
        unquoted(&calls[0].text()).contains("cluster reach 100.101.102.7 47823"),
        "{}",
        calls[0].text()
    );
}

#[test]
fn reach_fails_on_exit_one_with_the_line() {
    let rig = Rig::new(node_linux());
    rig.runner.on_script(
        "cluster reach",
        result(1, "no answer from 100.101.102.7:47823\n", ""),
    );
    let r = reach_check(&rig, Fake::new().primary("100.101.102.7", 47823));
    assert!(r.state.is_fail(), "{:?}", r.state);
    assert!(r.state.text().contains("no answer"), "{:?}", r.state);
    let rig = Rig::new(node_linux());
    rig.runner
        .on_script("cluster reach", result(2, "", "usage"));
    let r = reach_check(&rig, Fake::new().primary("100.101.102.7", 47823));
    assert!(!r.state.is_pass() && !r.state.is_fail(), "{:?}", r.state);
}

#[test]
fn reach_cant_tell_without_primary_addr() {
    let rig = Rig::new(node_linux());
    let r = reach_check(&rig, Fake::new());
    assert!(!r.state.is_pass() && !r.state.is_fail(), "{:?}", r.state);
    assert!(rig.calls().is_empty());
}

#[test]
fn reach_refuses_odd_addr_from_status() {
    for (addr, port) in [
        ("100.101.102.7; reboot", 47823),
        ("$(id)", 47823),
        ("192.168.1.1", 47823),
        ("100.128.0.1", 47823),
        ("fd7a:115c:a1e0::1", 47823),
        ("100.101.102.7", 0),
    ] {
        let rig = Rig::new(node_linux());
        let r = reach_check(&rig, Fake::new().primary(addr, port));
        assert!(
            !r.state.is_pass() && !r.state.is_fail(),
            "{addr}:{port} {:?}",
            r.state
        );
        assert!(rig.calls().is_empty(), "{addr}:{port}");
    }
}

#[test]
fn reach_sits_after_the_service_and_before_the_join() {
    let m = node_linux();
    let ids: Vec<StepId> = super::catalog::steps(&m, std::slice::from_ref(&m))
        .into_iter()
        .map(|s| s.id)
        .collect();
    let at = ids.iter().position(|i| *i == StepId::NODE_SERVICE).unwrap();
    assert_eq!(ids[at + 1], StepId::NODE_REACH);
    assert_eq!(ids[at + 2], StepId::NODE_JOINED);
    let joined = step(&m, &StepId::NODE_JOINED);
    assert!(joined.needs.contains(&StepId::NODE_REACH));
}

fn status_json(cluster: Option<&str>, me: &str) -> String {
    let cluster = cluster
        .map(|c| format!(r#"{{"id": "{c}", "name": "home", "term": 1}}"#))
        .unwrap_or_else(|| "null".into());
    format!("{{\"cluster\": {cluster}, \"me\": \"{me}\", \"members\": []}}")
}

fn joined_check(text: &str) -> CheckResult {
    let rig = Rig::new(node_linux());
    let (_, parse) = remote(&step(&rig.machine, &StepId::NODE_JOINED));
    let mut ctx = rig.step();
    ctx.tokens = Arc::new(Fake::new());
    parse(
        &RemoteOutput {
            text: text.into(),
            code: 0,
        },
        &ctx,
    )
}

#[test]
fn joined_reports_me_before_joining() {
    let none = joined_check(&status_json(None, ME));
    assert!(none.state.is_fail(), "{:?}", none.state);
    assert_eq!(none.detected.node.as_deref(), Some(ME));
    let other = joined_check(&status_json(Some("cluster-bbbb"), ME));
    assert!(other.state.is_fail(), "{:?}", other.state);
    assert_eq!(other.detected.node.as_deref(), Some(ME));
    let odd = joined_check(&status_json(None, "not an id; rm -rf"));
    assert!(odd.detected.node.is_none());
}

fn rig_with_status(me: &str) -> Rig {
    let rig = Rig::new(node_linux());
    rig.runner
        .on_script("cluster status --json", ok(&status_json(None, me)));
    rig
}

fn fix_with(rig: &Rig, fake: Arc<Fake>) -> FixContext {
    let mut ctx = rig.fix();
    ctx.step.tokens = fake;
    ctx.step.node = Some(STALE.into());
    ctx
}

fn run_join(ctx: &FixContext) -> super::step::FixOutcome {
    (plan(&step(&ctx.step.machine, &StepId::NODE_JOINED)).run)(ctx)
}

fn join_calls(rig: &Rig) -> Vec<super::runner::Recorded> {
    rig.calls()
        .into_iter()
        .filter(|c| c.text().contains("cluster join"))
        .collect()
}

#[test]
fn invite_token_only_on_stdin() {
    let rig = rig_with_status(ME);
    rig.runner.on_with(
        |a| a.last().is_some_and(|s| s.contains("cluster join")),
        CommandResult::new(2, "", &format!("the token {TOKEN} was refused\n")),
        vec![
            (Channel::Stdout, format!("joining with {TOKEN}\n")),
            (Channel::Stderr, format!("{TOKEN} was refused\n")),
        ],
    );
    let fake = Arc::new(Fake::new().answer(Ok(Some(TOKEN.into()))));
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = log.clone();
    let mut ctx = fix_with(&rig, fake.clone());
    ctx.log = Arc::new(move |_, t: &str| sink.lock().unwrap().push(t.to_string()));
    let out = run_join(&ctx);
    assert!(!out.ok);
    assert!(!out.message.contains(TOKEN), "{}", out.message);
    let calls = rig.calls();
    assert_eq!(join_calls(&rig).len(), 1);
    assert_eq!(join_calls(&rig)[0].stdin_text(), format!("{TOKEN}\n"));
    for c in &calls {
        assert!(!c.argv.join("\n").contains(TOKEN), "{:?}", c.argv);
        assert!(c.env.iter().all(|(_, v)| !v.contains(TOKEN)));
        if !c.text().contains("cluster join") {
            assert!(!c.stdin_text().contains(TOKEN));
        }
    }
    assert!(
        log.lock().unwrap().iter().all(|t| !t.contains(TOKEN)),
        "{:?}",
        log.lock().unwrap()
    );
    assert_eq!(fake.invites().len(), 1);
}

#[test]
fn join_rereads_me_before_invite() {
    let rig = rig_with_status(ME);
    rig.runner.on_script("cluster join", ok("Joined\n"));
    let fake = Arc::new(
        Fake::new()
            .watching(&rig.runner)
            .answer(Ok(Some(TOKEN.into()))),
    );
    let out = run_join(&fix_with(&rig, fake.clone()));
    assert!(out.ok, "{}", out.message);
    let calls = rig.calls();
    assert!(
        unquoted(&calls[0].text()).contains("cluster status --json"),
        "{}",
        calls[0].text()
    );
    assert_eq!(fake.seen.load(Ordering::SeqCst), 1);
    let invites = fake.invites();
    assert_eq!(invites.len(), 1);
    assert_eq!(invites[0].0, ME);
    assert_ne!(invites[0].0, STALE);
    assert_eq!(invites[0].1, "w1");
}

#[test]
fn join_refuses_an_odd_me() {
    for odd in [
        "ABCDEF0123456789",
        "0123456789abcde",
        "0123456789abcdef0",
        "not an id; rm -rf",
        "",
    ] {
        let rig = rig_with_status(odd);
        let fake = Arc::new(Fake::new().answer(Ok(Some(TOKEN.into()))));
        let out = run_join(&fix_with(&rig, fake.clone()));
        assert!(!out.ok, "{odd}");
        assert!(fake.invites().is_empty(), "{odd}");
        assert!(join_calls(&rig).is_empty(), "{odd}");
    }
    let rig = Rig::new(node_linux());
    rig.runner
        .on_script("cluster status --json", result(255, "", "ssh: no route"));
    let fake = Arc::new(Fake::new().answer(Ok(Some(TOKEN.into()))));
    let out = run_join(&fix_with(&rig, fake.clone()));
    assert!(!out.ok);
    assert!(fake.invites().is_empty());
}

#[test]
fn invite_carries_the_registry_choices() {
    let mut m = node_linux();
    m.node = Some(NodeSettings {
        choices: NodeChoices {
            can_be_master: true,
            acting: true,
        },
    });
    let rig = Rig::new(m);
    rig.runner
        .on_script("cluster status --json", ok(&status_json(None, ME)));
    rig.runner.on_script("cluster join", ok("Joined\n"));
    let fake = Arc::new(Fake::new().answer(Ok(Some(TOKEN.into()))));
    assert!(run_join(&fix_with(&rig, fake.clone())).ok);
    assert_eq!(fake.invites(), vec![(ME.into(), "w1".into(), true, true)]);
    let rig = rig_with_status(ME);
    rig.runner.on_script("cluster join", ok("Joined\n"));
    let fake = Arc::new(Fake::new().answer(Ok(Some(TOKEN.into()))));
    assert!(run_join(&fix_with(&rig, fake.clone())).ok);
    assert_eq!(fake.invites(), vec![(ME.into(), "w1".into(), false, false)]);
}

#[test]
fn machines_binary_falls_back_to_token() {
    let rig = rig_with_status(ME);
    rig.runner.on_script("cluster join", ok("Joined\n"));
    let fake = Arc::new(Fake::new().token(Ok(TOKEN.into())));
    let out = run_join(&fix_with(&rig, fake.clone()));
    assert!(out.ok, "{}", out.message);
    assert_eq!(fake.invites().len(), 1);
    let joins = join_calls(&rig);
    assert_eq!(joins.len(), 1);
    assert_eq!(joins[0].stdin_text(), format!("{TOKEN}\n"));
    for c in rig.calls() {
        assert!(!c.argv.join("\n").contains(TOKEN));
    }
}

#[test]
fn no_invite_and_no_token_says_to_use_the_app() {
    let rig = rig_with_status(ME);
    let fake =
        Arc::new(Fake::new().token(Err("only the Notchling app makes join tokens".to_string())));
    let out = run_join(&fix_with(&rig, fake));
    assert!(!out.ok);
    assert!(out.message.contains(NOT_APP), "{}", out.message);
    assert!(out.message.contains("Join Cluster"), "{}", out.message);
    assert!(out.message.contains("Machines sheet"), "{}", out.message);
    assert!(!out.message.contains("nlj1."), "{}", out.message);
    assert!(join_calls(&rig).is_empty());
}

#[test]
fn an_invite_error_stops_the_join() {
    let rig = rig_with_status(ME);
    let fake = Arc::new(Fake::new().answer(Err(format!("failed with {TOKEN}"))));
    let mut ctx = fix_with(&rig, fake);
    ctx.masks.hide("join-token", TOKEN);
    let out = run_join(&ctx);
    assert!(!out.ok);
    assert!(!out.message.contains(TOKEN), "{}", out.message);
    assert!(join_calls(&rig).is_empty());
}

fn join_answers(rig: &Rig, first: CommandResult, second: CommandResult) {
    let n = Arc::new(AtomicUsize::new(0));
    rig.runner.on_call(
        |a| a.last().is_some_and(|s| s.contains("cluster join")),
        move |_| {
            if n.fetch_add(1, Ordering::SeqCst) == 0 {
                first.clone()
            } else {
                second.clone()
            }
        },
    );
}

#[test]
fn expired_invite_retries_once() {
    let rig = rig_with_status(ME);
    join_answers(
        &rig,
        result(1, "", "the primary refused: this invite has expired\n"),
        ok("Joined\n"),
    );
    let fake = Arc::new(
        Fake::new()
            .answer(Ok(Some(TOKEN.into())))
            .answer(Ok(Some(OTHER_TOKEN.into()))),
    );
    let out = run_join(&fix_with(&rig, fake.clone()));
    assert!(out.ok, "{}", out.message);
    assert_eq!(fake.invites().len(), 2);
    let joins = join_calls(&rig);
    assert_eq!(joins.len(), 2);
    assert_eq!(joins[0].stdin_text(), format!("{TOKEN}\n"));
    assert_eq!(joins[1].stdin_text(), format!("{OTHER_TOKEN}\n"));
    for c in rig.calls() {
        let argv = c.argv.join("\n");
        assert!(!argv.contains(TOKEN) && !argv.contains(OTHER_TOKEN));
    }

    let rig = rig_with_status(ME);
    join_answers(
        &rig,
        result(1, "", "bad token\n"),
        result(1, "", "bad token\n"),
    );
    let fake = Arc::new(
        Fake::new()
            .answer(Ok(Some(TOKEN.into())))
            .answer(Ok(Some(OTHER_TOKEN.into())))
            .answer(Ok(Some("nlj1.THIRDTOKEN".into()))),
    );
    let out = run_join(&fix_with(&rig, fake.clone()));
    assert!(!out.ok);
    assert_eq!(fake.invites().len(), 2);
    assert_eq!(join_calls(&rig).len(), 2);
}

#[test]
fn other_failures_do_not_retry() {
    let rig = rig_with_status(ME);
    join_answers(
        &rig,
        result(1, "", "this token was made for another machine\n"),
        ok("Joined\n"),
    );
    let fake = Arc::new(
        Fake::new()
            .answer(Ok(Some(TOKEN.into())))
            .answer(Ok(Some(OTHER_TOKEN.into()))),
    );
    let out = run_join(&fix_with(&rig, fake.clone()));
    assert!(!out.ok);
    assert_eq!(fake.invites().len(), 1);
    assert_eq!(join_calls(&rig).len(), 1);
}

#[test]
fn an_old_registry_file_still_loads_with_acting_off() {
    let old = r#"{"name":"w1","host":"w1.test","user":"agent","node":{}}"#;
    let m: Machine = serde_json::from_str(old).unwrap();
    assert_eq!(m.node, Some(NodeSettings::default()));
    let choices = m.node.as_ref().unwrap().choices;
    assert!(!choices.acting && !choices.can_be_master);
    assert_eq!(
        serde_json::to_string(&m)
            .unwrap()
            .matches("choices")
            .count(),
        0
    );
    let chosen =
        r#"{"name":"w1","host":"w1.test","user":"agent","node":{"choices":{"acting":true}}}"#;
    let m: Machine = serde_json::from_str(chosen).unwrap();
    let c = m.node.as_ref().unwrap().choices;
    assert!(c.acting && !c.can_be_master);
    let back: Machine = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert_eq!(back, m);
}
