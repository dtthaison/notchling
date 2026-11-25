use super::addresses::{AddressKind, MachineAddress, Route};
use super::reach::*;
use super::registry::{Machine, REFUSED_HOST};
use super::runner::CommandResult;
use super::shell::ssh_script;
use super::tests_reach::{lan, routed_world, ts, with, Rig, PERSON, WORK};

fn touches(rig: &Rig, host: &str) -> bool {
    rig.runner
        .calls()
        .iter()
        .any(|c| c.argv.iter().any(|a| a.contains(host)))
}

fn jump(host: &str, through: &str) -> MachineAddress {
    MachineAddress::new(AddressKind::Jump, host).through(through)
}

#[test]
fn a_wrong_network_pick_refuses_instead_of_connecting_to_the_host_anyway() {
    let rig = Rig::new(&[], &[]);
    let m = with("m1", vec![ts("100.64.0.11", WORK)]);
    let out = rig.router.reached(&m, std::slice::from_ref(&m));
    assert_eq!(
        out.refused.as_deref(),
        Some("On work.example, and this Mac is on person@example.com")
    );
    assert_eq!(out.route().host, REFUSED_HOST);
    let argv = ssh_script(&out, "true");
    assert!(argv.iter().all(|a| !a.contains("100.64.0.11")), "{argv:?}");
    assert!(!touches(&rig, "100.64.0.11"));
    let again = rig.router.routed(&m);
    assert_eq!(again.refused, out.refused);
    rig.router.forget("m1");
    let cold = rig.router.routed(&m);
    assert!(cold.refused.is_none(), "a cold cache refuses nothing yet");
    assert_eq!(cold.route().host, REFUSED_HOST);
}

#[test]
fn an_unreachable_pick_does_not_drop_the_port_or_the_jump() {
    let rig = Rig::new(&["10.0.0.7"], &[]);
    let gateway = with("gw", vec![lan("10.0.0.1")]);
    rig.runner.on(
        |a| a.iter().any(|x| x == "agent@10.0.0.1"),
        CommandResult::new(255, "", "ssh: connect to host 10.0.0.1: No route to host"),
    );
    let behind = with(
        "m2",
        vec![lan("10.0.0.7").with_port(2222), jump("10.9.0.7", "gw")],
    );
    let out = rig.router.reached(&behind, &[gateway, behind.clone()]);
    assert_eq!(out.refused.as_deref(), Some("No address answered"));
    let argv = ssh_script(&out, "true");
    assert!(argv.iter().all(|a| !a.contains("10.0.0.7")), "{argv:?}");
    assert!(argv.iter().any(|a| a.contains(REFUSED_HOST)));
}

#[test]
fn a_machine_with_addresses_and_no_pick_yet_has_no_connecting_route() {
    let m = with("m1", vec![lan("192.168.1.20").with_port(2222)]);
    assert_eq!(m.route().host, REFUSED_HOST);
    let plain = Machine::new("m3", "m3.example", "agent");
    assert_eq!(plain.route().host, "m3.example");
}

#[test]
fn an_invalid_address_is_never_put_on_a_command_line() {
    let rig = Rig::new(&[], &[]);
    let m = with(
        "m1",
        vec![
            lan("-oProxyCommand=x"),
            lan("a,b"),
            lan("192.168.1.20").with_user("Bad User"),
        ],
    );
    assert_eq!(
        rig.router.pick(&m, std::slice::from_ref(&m)),
        Pick::Unreachable
    );
    assert!(rig.probes().is_empty());
    let gateway = with("gw", vec![lan("evil,host")]);
    let behind = with("m2", vec![jump("10.9.0.7", "gw")]);
    assert_eq!(
        rig.router.pick(&behind, &[gateway, behind.clone()]),
        Pick::Unreachable
    );
    assert!(rig.probes().is_empty());
}

#[test]
fn a_machine_with_an_invalid_address_is_refused_by_the_steps() {
    let mut m = with("m1", vec![lan("192.168.1.20")]);
    m.addresses.push(lan("a,b"));
    let why = super::steps_node::machine_refusal(&m).unwrap();
    assert!(why.starts_with("won't run on m1: "), "{why}");
}

#[test]
fn editing_or_forgetting_the_gateway_drops_the_routes_that_go_through_it() {
    let rig = Rig::new(&["10.0.0.50"], &[]);
    let gateway = with("gw", vec![lan("10.0.0.1"), lan("10.0.0.2")]);
    let behind = with("m2", vec![lan("10.0.0.50"), jump("10.9.0.7", "gw")]);
    let all = [gateway.clone(), behind.clone()];
    assert!(rig.router.pick(&behind, &all).choice().is_some());
    assert!(rig.router.cached(&behind).is_some());
    rig.router.forget("gw");
    assert_eq!(rig.router.cached(&behind), None);

    rig.router.pick(&behind, &all);
    let before = rig.probes().len();
    rig.router.pick(&behind, &all);
    assert_eq!(
        rig.probes().len(),
        before,
        "kept while the gateway is as it was"
    );
    let mut edited = gateway.clone();
    edited.set_addresses(vec![lan("10.0.0.2"), lan("10.0.0.1")]);
    rig.router.pick(&behind, &[edited, behind.clone()]);
    assert!(
        rig.probes().len() > before,
        "a changed gateway is tried again"
    );
}

#[test]
fn a_gateways_network_is_rechecked_for_the_machine_behind_it() {
    let rig = Rig::new(&["10.0.0.50"], &[]);
    let gateway = with("gw", vec![ts("gw.tail", PERSON), lan("10.0.0.1")]);
    let behind = with("m2", vec![lan("10.0.0.50"), jump("10.9.0.7", "gw")]);
    let all = [gateway, behind.clone()];
    rig.router.pick(&behind, &all);
    let listed = rig.lists();
    rig.at(RECHECK_NETWORK_SECS + 1.0);
    rig.router.pick(&behind, &all);
    assert!(rig.lists() > listed);
}

#[test]
fn a_jump_loop_ends_at_once_and_runs_few_probes() {
    let rig = Rig::new(&["10.0.0.1", "10.0.0.2", "10.0.0.3"], &[]);
    let node = |name: &str, last: u8, via: &str| {
        with(
            name,
            vec![lan(&format!("10.0.0.{last}")), jump("10.9.0.1", via)],
        )
    };
    let all = [node("a", 1, "b"), node("b", 2, "c"), node("c", 3, "a")];
    assert_eq!(rig.router.pick(&all[0], &all), Pick::Unreachable);
    assert!(rig.probes().len() <= 6, "{}", rig.probes().len());
}

#[test]
fn a_jump_host_with_one_address_is_probed_even_alone() {
    let rig = Rig::new(&["10.0.0.1"], &[]);
    let gateway = with("gw", vec![lan("10.0.0.1")]);
    let behind = with("m2", vec![jump("10.9.0.7", "gw")]);
    assert_eq!(
        rig.router.pick(&behind, &[gateway, behind.clone()]),
        Pick::Unreachable
    );
}

#[test]
fn a_cold_machine_lookup_runs_no_command() {
    let w = routed_world(&[]);
    let before = w.runner.calls().len();
    let found = w.engine.machine("m1").unwrap();
    assert_eq!(w.runner.calls().len(), before);
    assert_eq!(found.route().host, REFUSED_HOST);
}

#[test]
fn the_first_addresses_user_becomes_the_machines_user() {
    let mut m = Machine::new("m1", "old.example", "deploy");
    m.set_addresses(vec![
        lan("10.0.0.1").with_user("root"),
        lan("10.0.0.2"),
        lan("10.0.0.3").with_user("ci"),
    ]);
    assert_eq!(m.user, "root");
    assert_eq!(m.host, "10.0.0.1");
    assert_eq!(m.addresses[0].user, None);
    assert_eq!(m.addresses[1].user.as_deref(), Some("deploy"));
    assert_eq!(m.addresses[2].user.as_deref(), Some("ci"));
    let mut same = Machine::new("m2", "h", "deploy");
    same.set_addresses(vec![lan("10.0.0.1").with_user("deploy")]);
    assert_eq!(same.user, "deploy");
    assert_eq!(same.addresses[0].user, None);
}

#[test]
fn the_sudo_ssh_keeps_the_addresses_own_user() {
    let mut m = Machine::new("m1", "10.0.0.1", "deploy");
    m.route = Some(Route::direct("10.0.0.2", "ci"));
    let argv = super::shell::ssh_admin(&m, "true", false);
    let at = argv.iter().position(|a| a == "--").unwrap();
    assert_eq!(argv[at + 1], "ci@10.0.0.2");
    let as_root = super::shell::ssh_admin(&m, "true", true);
    let at = as_root.iter().position(|a| a == "--").unwrap();
    assert_eq!(as_root[at + 1], "root@10.0.0.2");
}

#[test]
fn an_ipv6_scp_host_is_bracketed() {
    assert_eq!(Route::direct("fd7a::1", "me").scp_host(), "me@[fd7a::1]");
    assert_eq!(Route::direct("a.example", "me").scp_host(), "me@a.example");
}

#[test]
fn the_cli_is_found_on_the_path_when_the_app_is_missing() {
    use super::tailscale::{resolve_cli, APP_CLI};
    use std::path::Path;
    let path = std::ffi::OsString::from("/nope:/opt/tools/bin");
    let only = |p: &Path| p == Path::new("/opt/tools/bin/tailscale");
    assert_eq!(resolve_cli(&path, &only), "/opt/tools/bin/tailscale");
    let app = |p: &Path| p == Path::new(APP_CLI) || p == Path::new("/opt/tools/bin/tailscale");
    assert_eq!(resolve_cli(&path, &app), APP_CLI);
    let none = |_: &Path| false;
    assert_eq!(resolve_cli(&path, &none), APP_CLI);
    let relative = std::ffi::OsString::from("bin:.");
    let rel = |p: &Path| p.is_relative();
    assert_eq!(resolve_cli(&relative, &rel), APP_CLI);
    let brew = |p: &Path| p == Path::new("/opt/homebrew/bin/tailscale");
    assert_eq!(resolve_cli(&path, &brew), "/opt/homebrew/bin/tailscale");
}
