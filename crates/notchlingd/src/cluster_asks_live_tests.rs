use super::*;
use crate::limits_ops_tests::TempDir;
use notchling_engine::machines::addresses::{AddressKind, MachineAddress};
use notchling_engine::machines::registry::REFUSED_HOST;
use notchling_engine::machines::roots::MachineRoots;
use notchling_engine::machines::runner::{CommandResult, ScriptedRunner};
use std::sync::Mutex;

const NEW: &str = "1111111111111111";

fn admin(tmp: &TempDir, runner: Arc<ScriptedRunner>) -> LiveAdmin {
    admin_with(
        tmp,
        runner,
        vec![Machine::new("newbox", "newbox.example.com", "me")],
    )
}

fn admin_with(tmp: &TempDir, runner: Arc<ScriptedRunner>, machines: Vec<Machine>) -> LiveAdmin {
    let dir = tmp.path("data");
    std::fs::create_dir_all(&dir).unwrap();
    let registry = MachineRoots::in_data(&dir).registry();
    assert!(registry.save(&machines));
    LiveAdmin::new(
        Err("no node".to_string()),
        registry,
        Organizations::new(&dir),
        runner,
    )
}

fn with_addresses(name: &str, list: Vec<MachineAddress>) -> Machine {
    let mut m = Machine::new(name, "", "me");
    m.set_addresses(list);
    m
}

fn lan(host: &str) -> MachineAddress {
    MachineAddress::new(AddressKind::Lan, host)
}

fn status_calls(runner: &ScriptedRunner) -> Vec<Vec<String>> {
    runner
        .calls()
        .into_iter()
        .map(|c| c.argv)
        .filter(|argv| argv.iter().any(|a| a.contains("cluster status --json")))
        .collect()
}

fn never_refused(runner: &ScriptedRunner) {
    for call in runner.calls() {
        assert!(
            call.argv.iter().all(|a| !a.contains(REFUSED_HOST)),
            "{:?}",
            call.argv
        );
    }
}

#[test]
fn a_machine_is_known_by_its_registry_name_and_its_own_node_id() {
    let tmp = TempDir::new("live-admin-node");
    let runner = Arc::new(ScriptedRunner::new());
    runner.on_script(
        "cluster status --json",
        CommandResult::ok(&format!("{{\"me\": \"{NEW}\"}}")),
    );
    let admin = admin(&tmp, runner.clone());
    assert_eq!(admin.matches("newbox"), 1);
    assert_eq!(admin.matches("NewBox"), 1);
    assert_eq!(admin.matches("ghost"), 0);
    assert_eq!(admin.node_of("newbox").as_deref(), Some(NEW));
    assert_eq!(admin.node_of("ghost"), None);
    assert_eq!(
        runner.calls().len(),
        1,
        "an unregistered name is never probed"
    );
}

#[test]
fn a_machine_that_does_not_answer_has_no_node() {
    let tmp = TempDir::new("live-admin-silent");
    let runner = Arc::new(ScriptedRunner::new());
    runner.on_script(
        "cluster status --json",
        CommandResult::new(255, "", "refused"),
    );
    assert_eq!(admin(&tmp, runner).node_of("newbox"), None);
}

#[test]
fn the_probe_is_capped_well_under_the_command_line_wait() {
    let tmp = TempDir::new("live-admin-cap");
    let runner = Arc::new(ScriptedRunner::new());
    let waits = Arc::new(Mutex::new(Vec::new()));
    let seen = waits.clone();
    runner.on_call(
        |a| {
            a.last()
                .is_some_and(|s| s.contains("cluster status --json"))
        },
        move |call| {
            seen.lock().unwrap().push(call.timeout);
            CommandResult::ok(&format!("{{\"me\": \"{NEW}\"}}"))
        },
    );
    assert_eq!(admin(&tmp, runner).node_of("newbox").as_deref(), Some(NEW));
    let waits = waits.lock().unwrap();
    assert_eq!(waits.len(), 1);
    assert!(waits[0] <= PROBE_CAP, "{:?}", waits[0]);
    assert!(waits[0] <= Duration::from_secs(8), "{:?}", waits[0]);
}

#[test]
fn a_name_in_machines_twice_has_no_owner_and_is_never_probed() {
    let tmp = TempDir::new("live-admin-twice");
    let runner = Arc::new(ScriptedRunner::new());
    let admin = admin(&tmp, runner.clone());
    let dir = tmp.path("data");
    let mut work = Machine::new("NEWBOX", "other.example.com", "me");
    work.organization = Some("work".to_string());
    assert!(MachineRoots::in_data(&dir)
        .registry()
        .save(&[Machine::new("newbox", "newbox.example.com", "me"), work]));
    assert_eq!(admin.matches("newbox"), 2);
    assert_eq!(admin.organization_of("newbox"), None);
    assert_eq!(admin.host_of("newbox"), None);
    assert_eq!(admin.node_of("newbox"), None);
    assert!(runner.calls().is_empty());
}

#[test]
fn this_macs_organization_is_personal_unless_the_document_is_unreadable() {
    let tmp = TempDir::new("live-admin-own");
    let ok = admin(&tmp, Arc::new(ScriptedRunner::new()));
    assert_eq!(ok.own_organization().as_deref(), Some("personal"));
    let broken = TempDir::new("live-admin-own-closed");
    let dir = broken.path("data");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(notchling_engine::store::DB_NAME), "not a database").unwrap();
    let closed = LiveAdmin::new(
        Err("no node".to_string()),
        MachineRoots::in_data(&dir).registry(),
        Organizations::new(&dir),
        Arc::new(ScriptedRunner::new()),
    );
    assert_eq!(closed.own_organization(), None);
}

#[test]
fn a_machine_with_addresses_is_probed_at_its_routed_host_never_at_refused_invalid() {
    let tmp = TempDir::new("live-admin-routed");
    let runner = Arc::new(ScriptedRunner::new());
    runner.on_script(
        "cluster status --json",
        CommandResult::ok(&format!("{{\"me\": \"{NEW}\"}}")),
    );
    let admin = admin_with(
        &tmp,
        runner.clone(),
        vec![with_addresses(
            "multibox",
            vec![lan("192.168.1.20").with_port(2222)],
        )],
    );
    assert_eq!(admin.host_of("multibox").as_deref(), Some("192.168.1.20"));
    assert!(runner.calls().is_empty(), "the stored facts need no ssh");
    assert_eq!(admin.node_of("multibox").as_deref(), Some(NEW));
    never_refused(&runner);
    let asked = status_calls(&runner);
    assert_eq!(asked.len(), 1);
    assert!(
        asked[0].iter().any(|a| a.contains("192.168.1.20")),
        "{:?}",
        asked[0]
    );
    assert!(asked[0].iter().any(|a| a == "2222"), "{:?}", asked[0]);
}

#[test]
fn the_first_address_that_answers_is_the_one_probed_for_the_node_id() {
    let tmp = TempDir::new("live-admin-second-address");
    let runner = Arc::new(ScriptedRunner::new());
    runner.on(
        |a| a.iter().any(|x| x.contains("192.168.1.20")) && a.last().is_some_and(|s| s == "true"),
        CommandResult::new(255, "", "ssh: connect to host: No route to host"),
    );
    runner.on_script(
        "cluster status --json",
        CommandResult::ok(&format!("{{\"me\": \"{NEW}\"}}")),
    );
    let admin = admin_with(
        &tmp,
        runner.clone(),
        vec![with_addresses(
            "multibox",
            vec![
                lan("192.168.1.20"),
                MachineAddress::new(AddressKind::Public, "203.0.113.5"),
            ],
        )],
    );
    assert_eq!(admin.node_of("multibox").as_deref(), Some(NEW));
    never_refused(&runner);
    let asked = status_calls(&runner);
    assert_eq!(asked.len(), 1);
    assert!(
        asked[0].iter().any(|a| a.contains("203.0.113.5")),
        "{:?}",
        asked[0]
    );
    assert!(
        asked[0].iter().all(|a| !a.contains("192.168.1.20")),
        "{:?}",
        asked[0]
    );
}

#[test]
fn no_ssh_call_waits_longer_than_the_cap_even_when_every_address_is_down() {
    let tmp = TempDir::new("live-admin-budget");
    let runner = Arc::new(ScriptedRunner::new());
    let waits = Arc::new(Mutex::new(Vec::new()));
    let seen = waits.clone();
    runner.on_call(
        |_| true,
        move |call| {
            seen.lock().unwrap().push(call.timeout);
            CommandResult::new(255, "", "No route to host")
        },
    );
    let admin = admin_with(
        &tmp,
        runner.clone(),
        vec![with_addresses(
            "multibox",
            vec![
                lan("192.168.1.20"),
                MachineAddress::new(AddressKind::Public, "203.0.113.5"),
            ],
        )],
    );
    assert_eq!(admin.node_of("multibox"), None);
    assert!(waits.lock().unwrap().iter().all(|w| *w <= PROBE_CAP));
    never_refused(&runner);
    assert!(
        status_calls(&runner).is_empty(),
        "no address answered, so nobody is asked who it is"
    );
}

fn fake_clock() -> (Clock, Arc<Mutex<f64>>) {
    let now = Arc::new(Mutex::new(0.0_f64));
    let read = now.clone();
    (Arc::new(move || *read.lock().unwrap()), now)
}

#[test]
fn time_one_call_spends_is_taken_from_the_next_and_nothing_runs_after_the_deadline() {
    let tmp = TempDir::new("live-admin-decay");
    let (clock, now) = fake_clock();
    let runner = Arc::new(ScriptedRunner::new());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (spent, record) = (now.clone(), seen.clone());
    runner.on_call(
        |_| true,
        move |call| {
            record.lock().unwrap().push(call.timeout);
            *spent.lock().unwrap() += 3.0;
            if call.argv.iter().any(|a| a.contains("192.168.1.20")) {
                CommandResult::new(255, "", "No route to host")
            } else {
                CommandResult::ok(&format!("{{\"me\": \"{NEW}\"}}"))
            }
        },
    );
    let admin = admin_with(
        &tmp,
        runner.clone(),
        vec![with_addresses(
            "multibox",
            vec![
                lan("192.168.1.20"),
                MachineAddress::new(AddressKind::Public, "203.0.113.5"),
            ],
        )],
    )
    .with_clock(clock);
    assert_eq!(admin.node_of("multibox"), None);
    assert_eq!(
        *seen.lock().unwrap(),
        vec![Duration::from_secs(5), Duration::from_secs(2)],
        "the second address check gets what the first left"
    );
    assert_eq!(
        runner.calls().len(),
        2,
        "the call after the deadline never reached the runner"
    );
    assert!(status_calls(&runner).is_empty());
}

#[test]
fn less_than_fifty_milliseconds_left_is_out_of_time() {
    let (clock, now) = fake_clock();
    let runner = Arc::new(ScriptedRunner::new());
    let budget = Budget {
        inner: runner.clone(),
        clock,
        until: 5.0,
    };
    let call = Call::secs(vec!["true".to_string()], 8);
    *now.lock().unwrap() = 4.96;
    let late = budget.run(&call);
    assert_eq!(late.code, 255);
    assert!(runner.calls().is_empty(), "40 ms is no time to start ssh");
    *now.lock().unwrap() = 4.94;
    budget.run(&call);
    assert_eq!(runner.calls().len(), 1);
}
