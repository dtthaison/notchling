use super::addresses::{AddressKind, MachineAddress, Route};
use super::reach::*;
use super::registry::Machine;
use super::runner::{CommandResult, Recorded, ScriptedRunner};
use super::shell::ssh_script;
use super::tailscale::Tailscale;
use super::tests_engine_world::{mac, step, Setup};
use super::tests_tailscale::SWITCH_LIST;
use std::sync::{Arc, Mutex};

pub(super) const PERSON: &str = "person@example.com";
pub(super) const WORK: &str = "work.example";

pub(super) const WORK_ACTIVE: &str = "ID    Tailnet              Account\na1b2  person@example.com   person@example.com\nc3d4  work.example         work@example.com*\n";

pub(super) struct Rig {
    pub(super) runner: Arc<ScriptedRunner>,
    pub(super) router: Router,
    pub(super) clock: Arc<Mutex<f64>>,
    pub(super) list: Arc<Mutex<&'static str>>,
}

pub(super) fn is_probe(r: &Recorded) -> bool {
    r.argv.first().map(String::as_str) == Some("/usr/bin/ssh")
        && r.argv.last().map(String::as_str) == Some("true")
}

pub(super) fn login(r: &Recorded) -> String {
    let at = r.argv.iter().position(|a| a == "--").unwrap();
    r.argv[at + 1].clone()
}

impl Rig {
    pub(super) fn new(down: &[&'static str], refused: &[&'static str]) -> Rig {
        let runner = Arc::new(ScriptedRunner::new());
        let list = Arc::new(Mutex::new(SWITCH_LIST));
        let shown = list.clone();
        runner.on_call(
            |a| a.get(1).map(String::as_str) == Some("switch"),
            move |_| CommandResult::ok(&shown.lock().unwrap()),
        );
        for &host in down {
            runner.on(
                move |a| {
                    a.last().map(String::as_str) == Some("true")
                        && a.iter().any(|x| x.ends_with(&format!("@{host}")))
                },
                CommandResult::new(
                    255,
                    "",
                    &format!("ssh: connect to host {host} port 22: Operation timed out"),
                ),
            );
        }
        for &host in refused {
            runner.on(
                move |a| {
                    a.last().map(String::as_str) == Some("true")
                        && a.iter().any(|x| x.ends_with(&format!("@{host}")))
                },
                CommandResult::new(255, "", "agent@host: Permission denied (publickey)."),
            );
        }
        let clock = Arc::new(Mutex::new(1_800_000_000.0));
        let c = clock.clone();
        let router = Router::new(
            runner.clone(),
            Tailscale::new(runner.clone(), "tailscale-fake"),
            Arc::new(move || *c.lock().unwrap()),
        );
        Rig {
            runner,
            router,
            clock,
            list,
        }
    }

    pub(super) fn at(&self, secs: f64) {
        *self.clock.lock().unwrap() += secs;
    }

    pub(super) fn probes(&self) -> Vec<Recorded> {
        self.runner.calls().into_iter().filter(is_probe).collect()
    }

    pub(super) fn lists(&self) -> usize {
        self.runner
            .calls()
            .iter()
            .filter(|c| c.argv.get(1).map(String::as_str) == Some("switch"))
            .count()
    }
}

pub(super) fn ts(host: &str, tailnet: &str) -> MachineAddress {
    MachineAddress::new(AddressKind::TailscaleName, host).on_tailnet(tailnet)
}

pub(super) fn lan(host: &str) -> MachineAddress {
    MachineAddress::new(AddressKind::Lan, host)
}

pub(super) fn with(name: &str, list: Vec<MachineAddress>) -> Machine {
    let mut m = Machine::new(name, "unused", "agent");
    m.set_addresses(list);
    m
}

#[test]
fn addresses_are_tried_in_order_and_the_first_that_answers_wins() {
    let rig = Rig::new(&["m1.tail"], &[]);
    let m = with(
        "m1",
        vec![
            ts("m1.tail", PERSON),
            lan("192.168.1.20").with_port(2222),
            lan("10.0.0.9"),
        ],
    );
    let pick = rig.router.pick(&m, std::slice::from_ref(&m));
    let choice = pick.choice().unwrap();
    assert_eq!(choice.index, 1);
    assert_eq!(
        choice.route,
        Route {
            host: "192.168.1.20".into(),
            user: "agent".into(),
            port: Some(2222),
            jump: None,
            key: None,
        }
    );
    let probes = rig.probes();
    assert_eq!(probes.len(), 2);
    assert_eq!(login(&probes[0]), "agent@m1.tail");
    assert_eq!(login(&probes[1]), "agent@192.168.1.20");
    assert_eq!(
        probes[1].argv,
        [
            "/usr/bin/ssh",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=3",
            "-p",
            "2222",
            "--",
            "agent@192.168.1.20",
            "true"
        ]
    );
}

#[test]
fn an_address_on_a_network_that_is_not_active_is_skipped_without_a_try() {
    let rig = Rig::new(&[], &[]);
    let m = with(
        "m1",
        vec![
            ts("m1.work.tail", WORK),
            ts("m1.tail", PERSON),
            lan("192.168.1.20"),
        ],
    );
    let pick = rig.router.pick(&m, std::slice::from_ref(&m));
    assert_eq!(pick.choice().unwrap().index, 1);
    let tried: Vec<String> = rig.probes().iter().map(login).collect();
    assert_eq!(tried, ["agent@m1.tail"]);
}

#[test]
fn a_tailscale_address_with_no_network_named_is_never_skipped() {
    let rig = Rig::new(&[], &[]);
    let m = with(
        "m1",
        vec![
            MachineAddress::new(AddressKind::TailscaleName, "m1.tail"),
            lan("192.168.1.20"),
        ],
    );
    assert_eq!(
        rig.router
            .pick(&m, std::slice::from_ref(&m))
            .choice()
            .unwrap()
            .index,
        0
    );
}

#[test]
fn only_addresses_on_another_network_say_so() {
    let rig = Rig::new(&[], &[]);
    let m = with("m1", vec![ts("m1.work.tail", WORK), ts("100.64.1.2", WORK)]);
    let pick = rig.router.pick(&m, std::slice::from_ref(&m));
    assert_eq!(
        pick,
        Pick::WrongNetwork {
            wanted: WORK.into(),
            active: PERSON.into()
        }
    );
    assert!(rig.probes().is_empty());
}

#[test]
fn a_dead_address_beside_one_on_another_network_is_still_the_wrong_network() {
    let rig = Rig::new(&["192.168.1.20"], &[]);
    let m = with(
        "m1",
        vec![
            ts("m1.work.tail", WORK),
            lan("192.168.1.20"),
            lan("10.0.0.9"),
        ],
    );
    rig.runner.on(
        |a| a.iter().any(|x| x.ends_with("@10.0.0.9")),
        CommandResult::new(255, "", "ssh: connect to host 10.0.0.9: No route to host"),
    );
    assert_eq!(
        rig.router.pick(&m, std::slice::from_ref(&m)),
        Pick::WrongNetwork {
            wanted: WORK.into(),
            active: PERSON.into()
        }
    );
}

#[test]
fn nothing_answering_with_nothing_skipped_is_unreachable() {
    let rig = Rig::new(&["192.168.1.20", "203.0.113.5"], &[]);
    let m = with(
        "m1",
        vec![
            lan("192.168.1.20"),
            MachineAddress::new(AddressKind::Public, "203.0.113.5"),
        ],
    );
    assert_eq!(
        rig.router.pick(&m, std::slice::from_ref(&m)),
        Pick::Unreachable
    );
}

#[test]
fn a_machine_that_refuses_us_beats_one_that_does_not_answer_but_not_one_that_works() {
    let rig = Rig::new(&["10.0.0.9"], &["192.168.1.20"]);
    let m = with(
        "m1",
        vec![lan("10.0.0.9"), lan("192.168.1.20"), lan("172.16.0.4")],
    );
    let pick = rig.router.pick(&m, std::slice::from_ref(&m));
    assert_eq!(pick.choice().unwrap().index, 2);
    rig.router.forget_all();
    let only_refused = with("m1", vec![lan("10.0.0.9"), lan("192.168.1.20")]);
    assert_eq!(
        rig.router
            .pick(&only_refused, std::slice::from_ref(&only_refused))
            .choice()
            .unwrap()
            .index,
        1
    );
}

#[test]
fn the_winner_is_kept_for_a_few_minutes_then_tried_again() {
    let rig = Rig::new(&["m1.tail"], &[]);
    let m = with("m1", vec![ts("m1.tail", PERSON), lan("192.168.1.20")]);
    let all = [m.clone()];
    let first = rig.router.pick(&m, &all);
    assert_eq!(rig.probes().len(), 2);
    rig.at(60.0);
    assert_eq!(rig.router.pick(&m, &all), first);
    assert_eq!(rig.probes().len(), 2, "still cached");
    rig.at(KEEP_SECS);
    rig.router.pick(&m, &all);
    assert_eq!(rig.probes().len(), 4, "tried again after the cache ran out");
}

#[test]
fn a_failure_is_kept_only_a_short_while() {
    let rig = Rig::new(&["192.168.1.20", "10.0.0.9"], &[]);
    let m = with("m1", vec![lan("192.168.1.20"), lan("10.0.0.9")]);
    let all = [m.clone()];
    assert_eq!(rig.router.pick(&m, &all), Pick::Unreachable);
    rig.at(KEEP_FAILED_SECS - 1.0);
    rig.router.pick(&m, &all);
    assert_eq!(rig.probes().len(), 2);
    rig.at(2.0);
    rig.router.pick(&m, &all);
    assert_eq!(rig.probes().len(), 4);
}

#[test]
fn editing_the_addresses_throws_the_kept_winner_away() {
    let rig = Rig::new(&[], &[]);
    let m = with("m1", vec![lan("192.168.1.20"), lan("10.0.0.9")]);
    rig.router.pick(&m, std::slice::from_ref(&m));
    let edited = with("m1", vec![lan("10.0.0.9"), lan("192.168.1.20")]);
    assert_eq!(rig.router.cached(&edited), None);
    let pick = rig.router.pick(&edited, std::slice::from_ref(&edited));
    assert_eq!(pick.choice().unwrap().route.host, "10.0.0.9");
}

#[test]
fn switching_the_network_makes_the_kept_winner_stale_after_a_moment() {
    let rig = Rig::new(&[], &[]);
    let m = with("m1", vec![ts("m1.tail", PERSON), ts("m1.work.tail", WORK)]);
    let all = [m.clone()];
    assert_eq!(rig.router.pick(&m, &all).choice().unwrap().index, 0);
    let lists = rig.lists();
    *rig.list.lock().unwrap() = WORK_ACTIVE;
    rig.at(2.0);
    assert_eq!(
        rig.router.pick(&m, &all).choice().unwrap().index,
        0,
        "not rechecked yet"
    );
    assert_eq!(rig.lists(), lists);
    rig.at(RECHECK_NETWORK_SECS);
    let pick = rig.router.pick(&m, &all);
    assert_eq!(pick.choice().unwrap().index, 1);
    assert!(rig.lists() > lists);
}

#[test]
fn a_single_usable_address_is_taken_without_a_try() {
    let rig = Rig::new(&[], &[]);
    let m = with("m1", vec![lan("192.168.1.20")]);
    let pick = rig.router.pick(&m, std::slice::from_ref(&m));
    assert_eq!(pick.choice().unwrap().index, 0);
    assert!(rig.probes().is_empty());
}

#[test]
fn a_machine_with_no_addresses_costs_no_call_at_all() {
    let rig = Rig::new(&[], &[]);
    let m = Machine::new("m1", "m1.tail", "agent");
    let pick = rig.router.pick(&m, std::slice::from_ref(&m));
    assert_eq!(
        pick.choice().unwrap().route,
        Route::direct("m1.tail", "agent")
    );
    assert!(rig.runner.calls().is_empty());
    assert_eq!(rig.router.routed(&m), m);
    assert_eq!(rig.router.reached(&m, std::slice::from_ref(&m)), m);
}

#[test]
fn a_jump_goes_through_the_other_machines_picked_address() {
    let rig = Rig::new(&["m1.tail"], &[]);
    let gateway = with(
        "m1",
        vec![ts("m1.tail", PERSON), lan("192.168.1.20").with_port(2222)],
    );
    let behind = with(
        "m2",
        vec![
            lan("10.0.0.50"),
            MachineAddress::new(AddressKind::Jump, "10.9.0.7").through("m1"),
        ],
    );
    rig.runner.on(
        |a| {
            a.last().map(String::as_str) == Some("true") && a.iter().any(|x| x == "agent@10.0.0.50")
        },
        CommandResult::new(
            255,
            "",
            "ssh: connect to host 10.0.0.50: Operation timed out",
        ),
    );
    let all = [gateway.clone(), behind.clone()];
    let pick = rig.router.pick(&behind, &all);
    let choice = pick.choice().unwrap();
    assert_eq!(choice.index, 1);
    let jump = choice.route.jump.as_deref().unwrap();
    assert_eq!(jump.host, "192.168.1.20");
    assert_eq!(jump.port, Some(2222));
    let probe = rig
        .probes()
        .into_iter()
        .find(|p| login(p) == "agent@10.9.0.7")
        .unwrap();
    assert_eq!(
        probe.argv,
        [
            "/usr/bin/ssh",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=3",
            "-J",
            "agent@192.168.1.20:2222",
            "--",
            "agent@10.9.0.7",
            "true"
        ]
    );
    let routed = rig.router.reached(&behind, &all);
    let argv = ssh_script(&routed, "echo hi");
    assert_eq!(argv[5], "-J");
    assert_eq!(argv[6], "agent@192.168.1.20:2222");
    assert_eq!(argv[7], "--");
}

#[test]
fn a_jump_through_a_machine_that_cannot_be_reached_is_not_an_address() {
    let rig = Rig::new(&["192.168.1.20", "10.0.0.50"], &[]);
    let gateway = with("m1", vec![lan("192.168.1.20"), lan("172.16.0.1")]);
    rig.runner.on(
        |a| a.iter().any(|x| x == "agent@172.16.0.1"),
        CommandResult::new(255, "", "ssh: connect to host 172.16.0.1: No route to host"),
    );
    let behind = with(
        "m2",
        vec![
            lan("10.0.0.50"),
            MachineAddress::new(AddressKind::Jump, "10.9.0.7").through("m1"),
        ],
    );
    assert_eq!(
        rig.router.pick(&behind, &[gateway, behind.clone()]),
        Pick::Unreachable
    );
    let alone = with(
        "m3",
        vec![
            lan("10.0.0.51"),
            MachineAddress::new(AddressKind::Jump, "10.9.0.7").through("nobody"),
        ],
    );
    rig.runner.on(
        |a| a.iter().any(|x| x == "agent@10.0.0.51"),
        CommandResult::new(
            255,
            "",
            "ssh: connect to host 10.0.0.51: Operation timed out",
        ),
    );
    assert_eq!(
        rig.router.pick(&alone, std::slice::from_ref(&alone)),
        Pick::Unreachable
    );
}

#[test]
fn two_machines_that_jump_through_each_other_end() {
    let rig = Rig::new(&["10.0.0.1", "10.0.0.2"], &[]);
    let a = with(
        "a",
        vec![
            lan("10.0.0.1"),
            MachineAddress::new(AddressKind::Jump, "10.9.0.1").through("b"),
        ],
    );
    let b = with(
        "b",
        vec![
            lan("10.0.0.2"),
            MachineAddress::new(AddressKind::Jump, "10.9.0.2").through("a"),
        ],
    );
    assert_eq!(
        rig.router.pick(&a, &[a.clone(), b.clone()]),
        Pick::Unreachable
    );
}

#[test]
fn the_cache_only_view_never_runs_a_command() {
    let rig = Rig::new(&[], &[]);
    let m = with("m1", vec![lan("192.168.1.20"), lan("10.0.0.9")]);
    assert_eq!(rig.router.routed(&m).route, None);
    assert!(rig.runner.calls().is_empty());
    rig.router.pick(&m, std::slice::from_ref(&m));
    let calls = rig.runner.calls().len();
    assert_eq!(rig.router.routed(&m).route.unwrap().host, "192.168.1.20");
    assert_eq!(rig.runner.calls().len(), calls);
}

#[test]
fn a_router_that_is_off_uses_the_first_address_and_runs_nothing() {
    let router = Router::off();
    let m = with("m1", vec![lan("192.168.1.20"), lan("10.0.0.9")]);
    assert_eq!(router.pick(&m, &[]).choice().unwrap().index, 0);
    let reached = router.reached(&m, &[]);
    assert_eq!(reached.route, Some(Route::direct("192.168.1.20", "agent")));
    assert_eq!(reached.refused, None);
    assert!(router.tailscale().is_none());
}

pub(super) fn routed_world(down: &[&str]) -> super::tests_engine_world::World {
    let mut m = mac("m1");
    m.set_addresses(vec![lan("10.0.0.5"), lan("10.0.0.6").with_port(2200)]);
    let mut setup = Setup::new(|_, _| vec![step("a")]);
    setup.machines = vec![m];
    setup.routed = true;
    for host in down {
        let host = host.to_string();
        setup.runner.on(
            move |a| {
                a.last().map(String::as_str) == Some("true")
                    && a.iter().any(|x| x.ends_with(&format!("@{host}")))
            },
            CommandResult::new(255, "", "ssh: connect to host: Operation timed out"),
        );
    }
    setup.build()
}

#[test]
fn a_check_runs_over_the_address_that_answered() {
    let w = routed_world(&["10.0.0.5"]);
    w.refresh("m1", false);
    let calls = w.runner.calls();
    let batch = calls
        .iter()
        .find(|c| {
            c.argv
                .last()
                .is_some_and(|s| s.contains("out=$( /bin/sh -c "))
        })
        .unwrap();
    assert_eq!(batch.argv[5..9], ["-p", "2200", "--", "worker@10.0.0.6"]);
    let tried: Vec<String> = calls.iter().filter(|c| is_probe(c)).map(login).collect();
    assert_eq!(tried, ["worker@10.0.0.5", "worker@10.0.0.6"]);
    w.refresh("m1", false);
    let again = w.runner.calls().iter().filter(|c| is_probe(c)).count();
    assert_eq!(again, 2, "the second check reuses the kept address");
}

#[test]
fn the_engine_reports_the_pick_it_made() {
    let w = routed_world(&["10.0.0.5"]);
    let machine = w.store.list.lock().unwrap()[0].clone();
    assert_eq!(w.engine.router().cached(&machine), None);
    w.refresh("m1", false);
    let pick = w.engine.router().cached(&machine).unwrap();
    assert_eq!(pick.choice().unwrap().route.host, "10.0.0.6");
}
