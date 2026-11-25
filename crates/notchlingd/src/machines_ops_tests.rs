use super::machine_asks_rig::{machine, on_tailscale, reason, rig, rig_standard};
use crate::consent_testkit::shell;
use notchling_local_api::{ops, ErrorCode};
use serde_json::json;

fn two_orgs(tag: &str) -> super::machine_asks_rig::Rig {
    rig(
        tag,
        vec![
            machine("mine", None),
            machine("theirs", Some("Work")),
            on_tailscale(machine("tsbox", None)),
        ],
    )
}

#[test]
fn list_shows_only_the_callers_organization() {
    let r = two_orgs("list");
    let reply = r.read(ops::MACHINES_LIST, json!({})).unwrap();
    let names: Vec<&str> = reply["machines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["mine", "tsbox"]);
    assert!(!reply.to_string().contains("theirs"));
}

#[test]
fn a_shell_caller_gets_no_session_on_every_read() {
    let r = two_orgs("shell");
    *r.who.lock().unwrap() = shell("Terminal");
    for op in [
        ops::MACHINES_LIST,
        ops::MACHINES_CHECK,
        ops::MACHINES_PEERS,
        ops::NETWORK_LIST,
    ] {
        let err = r.read(op, json!({})).unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden, "{op}");
        assert_eq!(reason(&err), "no_session", "{op}");
    }
    assert!(r.ssh.calls().is_empty() && r.tail.calls().is_empty());
}

#[test]
fn with_the_switch_off_check_gives_switch_off_and_no_ssh_runs() {
    let r = two_orgs("off");
    r.on.store(false, std::sync::atomic::Ordering::SeqCst);
    let err = r
        .read(ops::MACHINES_CHECK, json!({"machine": "mine"}))
        .unwrap_err();
    assert_eq!(reason(&err), "switch_off");
    assert_eq!(
        err.data.as_ref().unwrap()["ask"]["command"],
        "notchling switch on machines"
    );
    for op in [ops::MACHINES_LIST, ops::MACHINES_PEERS, ops::NETWORK_LIST] {
        assert_eq!(reason(&r.read(op, json!({})).unwrap_err()), "switch_off");
    }
    assert!(r.ssh.calls().is_empty() && r.tail.calls().is_empty());
}

#[test]
fn with_the_switch_on_check_runs_only_fast_probes() {
    let r = rig_standard("fast", vec![machine("mine", None)]);
    r.read(ops::MACHINES_CHECK, json!({"machine": "mine"}))
        .unwrap();
    assert!(!r.ssh.calls().is_empty(), "the fast probes did run");
    assert!(
        !r.ssh_text().contains("sudo -n -l"),
        "no slow probe: {}",
        r.ssh_text()
    );
}

#[test]
fn checking_another_organizations_machine_is_refused_without_a_probe() {
    let r = two_orgs("check-other");
    let err = r
        .read(ops::MACHINES_CHECK, json!({"machine": "theirs"}))
        .unwrap_err();
    assert_eq!(reason(&err), "other_organization");
    assert!(r.ssh.calls().is_empty());
}

#[test]
fn a_name_two_machines_share_has_an_unknown_owner() {
    let r = rig(
        "dupe",
        vec![machine("Box", None), machine("box", Some("Work"))],
    );
    let err = r
        .read(ops::MACHINES_CHECK, json!({"machine": "box"}))
        .unwrap_err();
    assert_eq!(reason(&err), "unknown_owner");
    assert!(r.ssh.calls().is_empty());
}

#[test]
fn check_marks_refused_steps_and_the_unknown_host_key() {
    let r = two_orgs("marks");
    let reply = r
        .read(ops::MACHINES_CHECK, json!({"machine": "mine"}))
        .unwrap();
    let steps = reply["machines"][0]["steps"].as_array().unwrap();
    assert!(!steps.is_empty());
    assert!(steps.iter().all(|s| s.get("agentMayFix").is_none()));
}

#[test]
fn check_marks_a_tailscale_sign_in_as_not_for_an_agent() {
    let r = two_orgs("sign-in-mark");
    r.sign_in.store(true, std::sync::atomic::Ordering::SeqCst);
    let reply = r
        .read(ops::MACHINES_CHECK, json!({"machine": "mine"}))
        .unwrap();
    let steps = reply["machines"][0]["steps"].as_array().unwrap();
    let sign_in = steps.iter().find(|s| s["id"] == "node-tailscale").unwrap();
    assert_eq!(sign_in["agentMayFix"], false);
    assert_eq!(sign_in["pointsAt"], "tailscale");
    let plain = steps.iter().find(|s| s["id"] == "plain").unwrap();
    assert!(plain.get("agentMayFix").is_none());
}

#[test]
fn peers_have_no_owner_and_no_tags() {
    let r = two_orgs("peers");
    let reply = r.read(ops::MACHINES_PEERS, json!({})).unwrap();
    let text = reply.to_string();
    assert!(text.contains("fresh"));
    assert!(!text.contains("friend@example.com"), "{text}");
    assert!(!text.contains("secret-role"), "{text}");
    assert!(!text.to_ascii_lowercase().contains("owner"), "{text}");
    assert!(!text.to_ascii_lowercase().contains("tags"), "{text}");
}

#[test]
fn network_list_masks_the_account_names() {
    let r = two_orgs("networks");
    let reply = r.read(ops::NETWORK_LIST, json!({})).unwrap();
    let text = reply.to_string();
    assert!(!text.contains("person@example.com"), "{text}");
    assert!(!text.contains("work@example.com"), "{text}");
    assert_eq!(reply["networks"].as_array().unwrap().len(), 2);
}

#[test]
fn the_plan_read_shows_a_plan_without_opening_a_card() {
    let r = two_orgs("plan");
    let reply = r
        .read(
            ops::MACHINES_PLAN,
            json!({"action": "network", "args": {"tailnet": "work.example"}}),
        )
        .unwrap();
    assert_eq!(reply["action"], "network");
    assert!(reply["change"].get("apply").is_none());
    assert!(r.rows().is_empty());
}

#[test]
fn a_check_after_a_fix_runs_no_slow_probe() {
    let r = two_orgs("slow-after-fix");
    let ran = || r.slow_runs.load(std::sync::atomic::Ordering::SeqCst);
    r.read(ops::MACHINES_CHECK, json!({"machine": "mine"}))
        .unwrap();
    assert_eq!(ran(), 0, "a check runs only the fast probes");
    let id = r.card("fix", json!({"machine": "mine", "step": "plain"}));
    r.apply(&id).unwrap();
    assert_eq!(ran(), 1, "the user's fix does check everything after it");
    *r.clock.lock().unwrap() += 700.0;
    r.read(ops::MACHINES_CHECK, json!({"machine": "mine"}))
        .unwrap();
    assert_eq!(ran(), 1, "an agent's check never runs the slow probes");
}
