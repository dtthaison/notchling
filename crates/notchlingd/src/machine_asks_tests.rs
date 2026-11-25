use super::machine_asks_rig::{machine, on_tailscale, reason, rig, with_entry, Rig, PASSWORD};
use crate::cited::Cited;
use crate::consent_testkit::{agent_caller, app_caller, shell};
use crate::op_group::OpGroup;
use notchling_engine::machines::claims::Claims;
use notchling_engine::machines::registry::{AdminAccess, MachineOs};
use notchling_local_api::{ops, ErrorCode, OpSpec};
use serde_json::{json, Value};

fn base(tag: &str) -> Rig {
    rig(
        tag,
        vec![
            machine("mine", None),
            machine("theirs", Some("Work")),
            on_tailscale(machine("tsbox", None)),
            on_tailscale(machine("workbox", Some("Work"))),
        ],
    )
}

fn everywhere(r: &Rig) -> String {
    fn walk(dir: &std::path::Path, out: &mut String) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if let Ok(bytes) = std::fs::read(&path) {
                out.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
    }
    let mut files = String::new();
    walk(r.tmp.root(), &mut files);
    format!(
        "{:?}|{}|{}|{:?}|{}",
        r.sink.events(),
        Value::Array(r.rows()),
        files,
        r.tail_calls(),
        r.ssh_text()
    )
}

#[test]
fn the_tiers_are_kept() {
    let r = base("tiers");
    assert_eq!(r.asks.spec(ops::MACHINES_REQUEST), Some(OpSpec::AGENT_ACTS));
    assert_eq!(r.asks.spec(ops::MACHINES_APPLY), Some(OpSpec::APP));
}

#[test]
fn a_shell_request_is_no_session() {
    let r = base("shell");
    *r.who.lock().unwrap() = shell("Terminal");
    let err = r
        .ask("add", json!({"address": "d@newbox.test"}))
        .unwrap_err();
    assert_eq!(reason(&err), "no_session");
    assert!(r.rows().is_empty());
}

#[test]
fn with_the_switch_off_a_request_says_switch_off_and_opens_nothing() {
    let r = base("off");
    r.on.store(false, std::sync::atomic::Ordering::SeqCst);
    let err = r
        .ask("add", json!({"address": "d@newbox.test"}))
        .unwrap_err();
    assert_eq!(reason(&err), "switch_off");
    assert_eq!(
        err.data.as_ref().unwrap()["ask"]["command"],
        "notchling switch on machines"
    );
    assert!(r.rows().is_empty() && r.ssh.calls().is_empty());
}

#[test]
fn another_organizations_machine_is_never_before_a_card() {
    let r = base("other");
    for (action, args) in [
        ("remove", json!({"machine": "theirs"})),
        ("fix", json!({"machine": "theirs", "step": "plain"})),
        ("setup", json!({"machine": "theirs"})),
        ("running", json!({"machine": "theirs", "on": true})),
        (
            "addresses",
            json!({"machine": "theirs", "addresses": ["x@1.2.3.4"]}),
        ),
    ] {
        let err = r.ask(action, args).unwrap_err();
        assert_eq!(reason(&err), "never", "{action}");
        assert_eq!(err.data.as_ref().unwrap()["never"], true);
    }
    assert!(r.rows().is_empty());
}

#[test]
fn an_unknown_owner_is_refused_before_a_card() {
    let r = rig(
        "unknown",
        vec![machine("Box", None), machine("box", Some("Work"))],
    );
    let err = r.ask("remove", json!({"machine": "box"})).unwrap_err();
    assert_eq!(reason(&err), "unknown_owner");
    assert!(r.rows().is_empty());
}

#[test]
fn user_only_actions_and_refused_steps_are_never() {
    let r = base("never");
    for action in ["trust", "trust_host", "login", "token", "join"] {
        let err = r.ask(action, json!({"machine": "mine"})).unwrap_err();
        assert_eq!(reason(&err), "never", "{action}");
    }
    for step in [
        "issues-token",
        "token-scope",
        "claude-login",
        "node-codex-login",
        "node-agy-login",
        "node-joined",
    ] {
        let err = r
            .ask("fix", json!({"machine": "mine", "step": step}))
            .unwrap_err();
        assert_eq!(reason(&err), "never", "{step}");
        assert!(
            err.data.as_ref().unwrap().get("pointsAt").is_some(),
            "{step}"
        );
    }
    assert!(r.rows().is_empty() && r.ssh.calls().is_empty());
}

#[test]
fn a_tailscale_sign_in_is_never_asked_for_by_fix_or_setup() {
    let r = base("sign-in-card");
    r.sign_in.store(true, std::sync::atomic::Ordering::SeqCst);
    checked(&r);
    let fix = r
        .ask("fix", json!({"machine": "mine", "step": "node-tailscale"}))
        .unwrap_err();
    let setup = r.ask("setup", json!({"machine": "mine"})).unwrap_err();
    for err in [fix, setup] {
        assert_eq!(reason(&err), "never");
        assert_eq!(err.data.as_ref().unwrap()["pointsAt"], "tailscale");
    }
    assert!(r.rows().is_empty() && r.fixed.lock().unwrap().is_empty());
}

#[test]
fn a_card_whose_step_became_a_tailscale_sign_in_is_stale_and_runs_nothing() {
    let r = base("sign-in-late");
    checked(&r);
    let id = r.card("setup", json!({"machine": "mine"}));
    r.sign_in.store(true, std::sync::atomic::Ordering::SeqCst);
    checked(&r);
    let err = r
        .apply_with(&id, Some(json!({"password": PASSWORD})))
        .unwrap_err();
    assert_eq!(reason(&err), "stale");
    assert!(r.fixed.lock().unwrap().is_empty());
}

#[test]
fn install_update_and_rename_are_not_built_yet() {
    let r = base("later");
    for action in ["install", "update", "rename"] {
        let err = r.ask(action, json!({"machine": "mine"})).unwrap_err();
        assert_eq!(err.code, ErrorCode::Unimplemented, "{action}");
        assert_eq!(reason(&err), "not_yet", "{action}");
    }
}

#[test]
fn add_writes_the_registry_once_on_apply_and_a_second_apply_is_refused() {
    let r = base("add");
    let before = r.registry_names();
    let id = r.card(
        "add",
        json!({"address": "deploy@newbox.test", "name": "newbox"}),
    );
    assert_eq!(r.registry_names(), before, "asking writes nothing");
    assert_eq!(*r.changes.lock().unwrap(), 0);
    r.apply(&id).unwrap();
    let after = r.registry_names();
    assert_eq!(after.iter().filter(|n| *n == "newbox").count(), 1);
    assert_eq!(*r.changes.lock().unwrap(), 1);
    let again = r.apply(&id).unwrap_err();
    assert_eq!(reason(&again), "needs_consent");
    assert_eq!(r.registry_names(), after);
    assert!(r.ssh.calls().is_empty(), "an add never reaches the machine");
}

#[test]
fn an_add_in_the_callers_organization_is_put_there() {
    let r = base("add-org");
    let id = r.card(
        "add",
        json!({"address": "deploy@newbox.test", "name": "newbox"}),
    );
    r.apply(&id).unwrap();
    let added = r
        .machines
        .all()
        .into_iter()
        .find(|m| m.name == "newbox")
        .unwrap();
    assert_eq!(added.organization_id(), "personal");
}

#[test]
fn a_dry_run_opens_no_card_and_writes_nothing() {
    let r = base("dry");
    let reply = r
        .asks
        .call(
            &agent_caller(),
            ops::MACHINES_REQUEST,
            &json!({"action": "add", "args": {"address": "d@newbox.test"}, "dryRun": true}),
        )
        .unwrap();
    assert_eq!(reply["dryRun"], true);
    assert!(r.rows().is_empty());
    assert!(!r.registry_names().contains(&"newbox".to_string()));
}

#[test]
fn the_same_key_gives_the_same_card_and_another_request_with_it_is_refused() {
    let r = base("key");
    let ask = |args: Value| {
        r.asks.call(
            &agent_caller(),
            ops::MACHINES_REQUEST,
            &json!({"action": "add", "args": args, "consent": {"key": "k1"}}),
        )
    };
    let first = ask(json!({"address": "d@newbox.test", "name": "newbox"})).unwrap();
    let second = ask(json!({"address": "d@newbox.test", "name": "newbox"})).unwrap();
    assert_eq!(first["request"]["id"], second["request"]["id"]);
    let other = ask(json!({"address": "d@other.test", "name": "other"})).unwrap_err();
    assert_eq!(reason(&other), "key_reused");
}

#[test]
fn removing_a_cluster_member_is_refused() {
    let r = base("member");
    *r.members.node.lock().unwrap() = Some(("mine".into(), "aaaa".into()));
    let err = r.ask("remove", json!({"machine": "mine"})).unwrap_err();
    assert_eq!(reason(&err), "cluster_member");
    assert!(r.rows().is_empty());
}

#[test]
fn removing_a_machine_drops_it_on_apply() {
    let r = base("remove");
    let id = r.card("remove", json!({"machine": "mine"}));
    assert!(r.registry_names().contains(&"mine".to_string()));
    r.apply(&id).unwrap();
    assert!(!r.registry_names().contains(&"mine".to_string()));
}

#[test]
fn addresses_are_replaced_on_apply() {
    let r = base("addresses");
    let id = r.card(
        "addresses",
        json!({"machine": "mine", "addresses": ["worker@192.168.1.20"]}),
    );
    r.apply(&id).unwrap();
    let m = r
        .machines
        .all()
        .into_iter()
        .find(|m| m.name == "mine")
        .unwrap();
    assert_eq!(m.address_list().len(), 1);
    assert_eq!(m.address_list()[0].host, "192.168.1.20");
}

fn checked(r: &Rig) {
    r.read(ops::MACHINES_CHECK, json!({"machine": "mine"}))
        .unwrap();
}

#[test]
fn a_fix_runs_its_step_on_apply_and_only_once() {
    let r = base("fix");
    checked(&r);
    let id = r.card("fix", json!({"machine": "mine", "step": "plain"}));
    assert!(r.fixed.lock().unwrap().is_empty(), "asking runs nothing");
    r.apply(&id).unwrap();
    assert_eq!(*r.fixed.lock().unwrap(), ["plain"]);
    assert!(r.apply(&id).is_err());
    assert_eq!(r.fixed.lock().unwrap().len(), 1);
}

#[test]
fn a_fix_whose_display_changed_is_withdrawn_stale_and_nothing_runs() {
    let r = base("stale");
    checked(&r);
    let id = r.card("fix", json!({"machine": "mine", "step": "plain"}));
    *r.shown.lock().unwrap() = "echo something else".to_string();
    let err = r.apply(&id).unwrap_err();
    assert_eq!(reason(&err), "stale");
    assert!(r.fixed.lock().unwrap().is_empty());
    let row = r.consent.get_row(&id).unwrap();
    assert_eq!(row.state.as_str(), "withdrawn");
}

#[test]
fn a_daemon_fix_is_refused_while_the_app_holds_the_claim() {
    let r = base("claim");
    checked(&r);
    let id = r.card("fix", json!({"machine": "mine", "step": "plain"}));
    let held = Claims::in_data(r.tmp.root()).take("mine").unwrap();
    let err = r.apply(&id).unwrap_err();
    assert_eq!(reason(&err), "busy");
    assert!(r.fixed.lock().unwrap().is_empty());
    drop(held);
}

#[test]
fn a_step_that_needs_the_admin_password_asks_for_it_before_the_card_is_used() {
    let r = base("admin-needed");
    checked(&r);
    let id = r.card("fix", json!({"machine": "mine", "step": "rooted"}));
    assert_eq!(r.consent.get_row(&id).unwrap().change["admin"], "password");
    let err = r.apply(&id).unwrap_err();
    assert_eq!(reason(&err), "admin_needed");
    assert_eq!(r.consent.get_row(&id).unwrap().state.as_str(), "waiting");
    r.apply_with(&id, Some(json!({"password": PASSWORD})))
        .unwrap();
    assert_eq!(*r.passwords.lock().unwrap(), [PASSWORD]);
}

#[test]
fn the_admin_password_is_in_no_row_event_audit_log_result_or_argv() {
    let r = base("secret");
    checked(&r);
    let id = r.card("fix", json!({"machine": "mine", "step": "rooted"}));
    let result = r
        .apply_with(&id, Some(json!({"password": PASSWORD})))
        .unwrap();
    assert!(!result.to_string().contains(PASSWORD));
    assert_eq!(
        *r.passwords.lock().unwrap(),
        [PASSWORD],
        "it did reach the step"
    );
    let seen = everywhere(&r);
    assert!(!seen.contains(PASSWORD), "the password leaked");
    let failed = r.card("fix", json!({"machine": "mine", "step": "rooted"}));
    let err = r.apply_with(&failed, Some(json!({"password": PASSWORD})));
    let shown = format!("{err:?}{}", everywhere(&r));
    assert!(!shown.contains(PASSWORD));
}

#[test]
fn an_agent_cannot_apply_and_an_unclaimed_apply_is_refused() {
    let r = base("apply");
    let id = r.card("add", json!({"address": "d@newbox.test", "name": "newbox"}));
    let err = r
        .asks
        .call(
            &agent_caller(),
            ops::MACHINES_APPLY,
            &json!({"consentId": id}),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    let err = r
        .asks
        .call(&app_caller(), ops::MACHINES_APPLY, &json!({}))
        .unwrap_err();
    assert_eq!(reason(&err), "needs_consent");
    assert!(!r.registry_names().contains(&"newbox".to_string()));
}

#[test]
fn a_cited_apply_by_an_agent_is_refused() {
    let r = base("agent-apply");
    let id = r.card("add", json!({"address": "d@newbox.test", "name": "newbox"}));
    let args = json!({"consentId": id});
    assert!(r.asks.claims(ops::MACHINES_APPLY, &args));
    let err = r
        .asks
        .apply(&agent_caller(), ops::MACHINES_APPLY, &args, &|_| {
            Ok(json!({}))
        })
        .unwrap_err();
    assert_eq!(reason(&err), "app_only");
}

#[test]
fn a_network_switch_runs_exactly_tailscale_switch_and_the_agent_sees_counts_only() {
    let r = base("network");
    let reply = r
        .ask("network", json!({"tailnet": "work.example"}))
        .unwrap();
    let id = reply["request"]["id"].as_str().unwrap().to_string();
    let agent_view = reply.to_string();
    for name in ["tsbox", "workbox", "theirs"] {
        assert!(!agent_view.contains(name), "{name} leaked: {agent_view}");
    }
    assert!(agent_view.contains("consent-network-drops"));
    let app_view = Value::Array(r.rows()).to_string();
    assert!(
        app_view.contains("tsbox") && app_view.contains("workbox"),
        "{app_view}"
    );
    let mine = r
        .consent
        .call(&agent_caller(), ops::CONSENT_MINE, &json!({}))
        .unwrap();
    assert!(!mine.to_string().contains("workbox"));
    r.apply(&id).unwrap();
    let switches: Vec<Vec<String>> = r
        .tail_calls()
        .into_iter()
        .filter(|a| {
            a.get(1).map(String::as_str) == Some("switch")
                && a.get(2).map(String::as_str) != Some("--list")
        })
        .collect();
    assert_eq!(switches, [["/fake/tailscale", "switch", "c3d4"]]);
    assert!(r.ssh.calls().is_empty());
}

#[test]
fn a_network_card_already_on_that_network_is_refused() {
    let r = base("network-same");
    let err = r
        .ask("network", json!({"tailnet": "person@example.com"}))
        .unwrap_err();
    assert_eq!(reason(&err), "already_set");
}

#[test]
fn a_switch_the_op_refuses_as_stale_withdraws_the_card_and_the_waiting_cli_sees_withdrawn() {
    let r = base("stale-switch");
    let id = r.card("network", json!({"tailnet": "work.example"}));
    let row = r.consent.get_row(&id).unwrap();
    let mut args = row.change["apply"]["args"].clone();
    args["consentId"] = json!(id);
    let stale = |_: &Value| {
        Err(
            notchling_local_api::ApiError::new(ErrorCode::Forbidden, "That name is taken now.")
                .with_reason("stale"),
        )
    };
    let err = r
        .asks
        .apply(&app_caller(), ops::TAILSCALE_SWITCH, &args, &stale)
        .unwrap_err();
    assert_eq!(reason(&err), "stale");
    let state = r.consent.get_row(&id).unwrap().state.as_str().to_string();
    assert_eq!(state, "withdrawn");
    let failure = crate::consent_cli::failure_of(&json!({"id": id, "state": state}), "consent.get");
    assert_eq!(failure.reason, "withdrawn");
}

#[test]
fn a_call_carrying_a_consent_id_is_claimed_whatever_the_id_and_never_reaches_the_raw_op() {
    let r = base("claims");
    let id = r.card("network", json!({"tailnet": "work.example"}));
    assert!(r.asks.claims(
        ops::TAILSCALE_SWITCH,
        &json!({"tailnet": "c3d4", "consentId": id})
    ));
    for op in [ops::MACHINES_APPLY, ops::TAILSCALE_SWITCH] {
        for bad in [json!("nope"), json!(7), json!(null), json!(true)] {
            assert!(r.asks.claims(op, &json!({"consentId": bad})), "{op} {bad}");
        }
        assert!(!r.asks.claims(op, &json!({"tailnet": "c3d4"})), "{op}");
    }
    assert!(!r
        .asks
        .claims(ops::MACHINES_LIST, &json!({"consentId": "x"})));
    let ran = std::sync::Arc::new(std::sync::Mutex::new(0));
    let count = ran.clone();
    let run = move |_: &Value| {
        *count.lock().unwrap() += 1;
        Ok(json!({}))
    };
    let gone = r
        .asks
        .apply(
            &app_caller(),
            ops::TAILSCALE_SWITCH,
            &json!({"tailnet": "c3d4", "consentId": "nope"}),
            &run,
        )
        .unwrap_err();
    assert_eq!(reason(&gone), "needs_consent");
    let typed = r
        .asks
        .apply(
            &app_caller(),
            ops::MACHINES_APPLY,
            &json!({"consentId": 7}),
            &run,
        )
        .unwrap_err();
    assert_eq!(typed.code, ErrorCode::BadRequest);
    assert_eq!(*ran.lock().unwrap(), 0);
    assert!(r
        .tail_calls()
        .iter()
        .all(|c| c.get(1).map(String::as_str) != Some("switch")
            || c.get(2).map(String::as_str) == Some("--list")));
}

#[test]
fn a_card_of_another_kind_cited_on_a_machines_op_is_refused_and_not_used() {
    let r = base("claims-kind");
    let other = r
        .consent
        .open(
            &agent_caller(),
            crate::consent_ops::Opening {
                parts: None,
                kind: notchling_engine::consent::SWITCH,
                target: "t".into(),
                request: json!({"op": "x"}),
                change: json!({"apply": {"op": ops::MACHINES_APPLY, "args": {}}}),
                facts: json!([]),
                reason: None,
                expires_in_ms: None,
                key: None,
            },
        )
        .unwrap();
    let id = other.row().id.clone();
    let err = r
        .asks
        .apply(
            &app_caller(),
            ops::MACHINES_APPLY,
            &json!({"consentId": id}),
            &|_| Ok(json!({})),
        )
        .unwrap_err();
    assert_eq!(reason(&err), "needs_consent");
    assert_eq!(r.consent.get_row(&id).unwrap().state.as_str(), "waiting");
}

#[test]
fn a_card_for_one_network_cannot_switch_to_another() {
    let r = base("network-other");
    let id = r.card("network", json!({"tailnet": "work.example"}));
    let err = r
        .asks
        .apply(
            &app_caller(),
            ops::TAILSCALE_SWITCH,
            &json!({"tailnet": "a1b2", "consentId": id}),
            &|_| Ok(json!({})),
        )
        .unwrap_err();
    assert_eq!(reason(&err), "needs_consent");
    assert!(r
        .tail_calls()
        .iter()
        .all(|a| a.get(2).map(String::as_str) != Some("a1b2")));
}

fn rooted_step(r: &Rig, machine: &str) -> String {
    r.card("fix", json!({"machine": machine, "step": "rooted"}))
}

fn drifting(r: &Rig, text: &str, after: usize) {
    let at = r.lists.load(std::sync::atomic::Ordering::SeqCst) + after;
    *r.drift.lock().unwrap() = Some((at, text.to_string()));
}

#[test]
fn a_setup_card_says_what_admin_access_its_steps_need_and_runs_them_with_it() {
    let r = base("setup-admin");
    checked(&r);
    let id = r.card("setup", json!({"machine": "mine"}));
    let row = r.consent.get_row(&id).unwrap();
    assert_eq!(row.change["admin"], "password");
    let need = row
        .facts
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["line"] == "consent-machine-admin");
    assert_eq!(need.unwrap()["args"]["need"], "password");
    let err = r.apply(&id).unwrap_err();
    assert_eq!(reason(&err), "admin_needed");
    assert_eq!(r.consent.get_row(&id).unwrap().state.as_str(), "waiting");
    assert!(r.fixed.lock().unwrap().is_empty());
    r.apply_with(&id, Some(json!({"password": PASSWORD})))
        .unwrap();
    assert_eq!(*r.fixed.lock().unwrap(), ["plain", "rooted"]);
    assert_eq!(*r.passwords.lock().unwrap(), [PASSWORD]);
}

#[test]
fn a_setup_card_with_no_step_that_needs_admin_says_none() {
    let r = base("setup-plain");
    checked(&r);
    r.fixed.lock().unwrap().push("rooted".into());
    checked(&r);
    let id = r.card("setup", json!({"machine": "mine"}));
    assert_eq!(r.consent.get_row(&id).unwrap().change["admin"], "none");
    r.apply(&id).unwrap();
    assert_eq!(*r.fixed.lock().unwrap(), ["rooted", "plain"]);
}

#[test]
fn a_plan_whose_steps_changed_after_the_card_is_stale_and_runs_nothing() {
    let r = base("digest");
    checked(&r);
    let id = r.card("setup", json!({"machine": "mine"}));
    drifting(&r, "echo drifted", 1);
    let err = r
        .apply_with(&id, Some(json!({"password": PASSWORD})))
        .unwrap_err();
    assert_eq!(reason(&err), "stale");
    assert!(r.fixed.lock().unwrap().is_empty());
    assert!(r.passwords.lock().unwrap().is_empty());
    assert_eq!(r.consent.get_row(&id).unwrap().state.as_str(), "withdrawn");
}

#[test]
fn a_fix_whose_steps_changed_after_the_card_is_stale_and_runs_nothing() {
    let r = base("digest-fix");
    checked(&r);
    let id = r.card("fix", json!({"machine": "mine", "step": "plain"}));
    drifting(&r, "echo drifted", 2);
    let err = r.apply(&id).unwrap_err();
    assert_eq!(reason(&err), "stale");
    assert!(r.fixed.lock().unwrap().is_empty());
}

fn vaulted(tag: &str) -> Rig {
    rig(
        tag,
        vec![
            with_entry(machine("vaulted", None), "vault-sudo"),
            machine("bare", None),
        ],
    )
}

#[test]
fn a_card_names_the_machines_saved_admin_entry_for_the_app_only() {
    let r = vaulted("entry-shown");
    r.read(ops::MACHINES_CHECK, json!({"machine": "vaulted"}))
        .unwrap();
    let asked = r
        .ask("fix", json!({"machine": "vaulted", "step": "rooted"}))
        .unwrap();
    assert!(!asked.to_string().contains("vault-sudo"), "{asked}");
    let id = asked["request"]["id"].as_str().unwrap();
    let seen = r.rows();
    let row = seen.iter().find(|x| x["id"] == id).unwrap();
    assert_eq!(row["change"]["adminEntry"], "vault-sudo", "{row}");
    let would = r
        .asks
        .call(
            &agent_caller(),
            ops::MACHINES_REQUEST,
            &json!({"action": "fix", "args": {"machine": "vaulted", "step": "rooted"}, "dryRun": true}),
        )
        .unwrap();
    assert!(!would.to_string().contains("vault-sudo"), "{would}");
    let plan = r
        .read(
            ops::MACHINES_PLAN,
            json!({"action": "fix", "args": {"machine": "vaulted", "step": "rooted"}}),
        )
        .unwrap();
    assert!(!plan.to_string().contains("vault-sudo"), "{plan}");
    r.read(ops::MACHINES_CHECK, json!({"machine": "bare"}))
        .unwrap();
    let bare = rooted_step(&r, "bare");
    assert!(r
        .consent
        .get_row(&bare)
        .unwrap()
        .change
        .get("adminEntry")
        .is_none());
}

#[test]
fn a_vault_pick_that_is_not_the_saved_entry_is_refused_and_keeps_the_card() {
    let r = vaulted("entry-wrong");
    r.read(ops::MACHINES_CHECK, json!({"machine": "vaulted"}))
        .unwrap();
    let id = rooted_step(&r, "vaulted");
    let err = r
        .apply_with(&id, Some(json!({"vault": "someone-elses"})))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
    assert_eq!(reason(&err), "vault_entry");
    assert_eq!(r.consent.get_row(&id).unwrap().state.as_str(), "waiting");
    assert!(r.fixed.lock().unwrap().is_empty());
    r.apply_with(&id, Some(json!({"vault": "vault-sudo"})))
        .unwrap();
    assert_eq!(*r.fixed.lock().unwrap(), ["rooted"]);
    assert_eq!(*r.passwords.lock().unwrap(), [PASSWORD]);
}

#[test]
fn a_saved_entry_name_with_spaces_is_read_from_the_vault_by_its_trimmed_name() {
    let r = rig(
        "entry-padded",
        vec![with_entry(machine("padded", None), "  Vault-Sudo ")],
    );
    r.read(ops::MACHINES_CHECK, json!({"machine": "padded"}))
        .unwrap();
    let id = rooted_step(&r, "padded");
    assert_eq!(
        r.consent.get_row(&id).unwrap().change["adminEntry"],
        "Vault-Sudo"
    );
    r.apply_with(&id, Some(json!({"vault": "vault-sudo"})))
        .unwrap();
    assert_eq!(*r.fixed.lock().unwrap(), ["rooted"]);
    assert_eq!(*r.passwords.lock().unwrap(), [PASSWORD]);
    assert_eq!(r.consent.get_row(&id).unwrap().state.as_str(), "applied");
}

#[test]
fn a_vault_pick_for_a_machine_with_no_saved_entry_is_refused_and_keeps_the_card() {
    let r = vaulted("entry-none");
    r.read(ops::MACHINES_CHECK, json!({"machine": "bare"}))
        .unwrap();
    let id = rooted_step(&r, "bare");
    let err = r
        .apply_with(&id, Some(json!({"vault": "vault-sudo"})))
        .unwrap_err();
    assert_eq!(reason(&err), "vault_entry");
    assert_eq!(r.consent.get_row(&id).unwrap().state.as_str(), "waiting");
    r.apply_with(&id, Some(json!({"password": PASSWORD})))
        .unwrap();
    assert_eq!(*r.fixed.lock().unwrap(), ["rooted"]);
}

#[test]
fn a_typed_login_user_other_than_the_machines_is_refused_and_a_plain_host_is_not() {
    let r = base("login-user");
    let err = r
        .ask(
            "addresses",
            json!({"machine": "mine", "addresses": ["root@192.168.1.20"]}),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert_eq!(reason(&err), "login_user");
    assert_eq!(err.data.as_ref().unwrap()["never"], true);
    let second = r
        .ask(
            "addresses",
            json!({"machine": "mine", "addresses": ["worker@10.0.0.1", "root@10.0.0.2"]}),
        )
        .unwrap_err();
    assert_eq!(reason(&second), "login_user");
    assert!(r.rows().is_empty());
    let id = r.card(
        "addresses",
        json!({"machine": "mine", "addresses": ["192.168.1.20", "worker@10.0.0.2"]}),
    );
    let row = r.consent.get_row(&id).unwrap();
    let line = row
        .facts
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["line"] == "consent-machine-addresses");
    assert_eq!(
        line.unwrap()["args"]["after"],
        "worker@192.168.1.20, worker@10.0.0.2"
    );
    r.apply(&id).unwrap();
    let m = r
        .machines
        .all()
        .into_iter()
        .find(|m| m.name == "mine")
        .unwrap();
    assert_eq!(m.user, "worker");
}

#[test]
fn a_setup_card_for_a_linux_machine_with_a_sudo_admin_and_an_admin_step_is_refused_before_a_card() {
    let mut linux = machine("lin", None);
    linux.os = Some(MachineOs::Linux);
    linux.set_admin(AdminAccess::Sudo);
    let r = rig("setup-linux-sudo", vec![linux]);
    r.read(ops::MACHINES_CHECK, json!({"machine": "lin"}))
        .unwrap();
    let err = r.ask("setup", json!({"machine": "lin"})).unwrap_err();
    assert_eq!(reason(&err), "refused");
    assert!(r.rows().is_empty());
    assert!(r.fixed.lock().unwrap().is_empty());
}

#[test]
fn the_cards_own_vault_entry_and_machine_are_accepted_whatever_their_case_or_spaces() {
    let r = vaulted("entry-case");
    r.read(ops::MACHINES_CHECK, json!({"machine": "vaulted"}))
        .unwrap();
    let id = rooted_step(&r, "VAULTED");
    r.apply_with(&id, Some(json!({"vault": " Vault-SUDO "})))
        .unwrap();
    assert_eq!(*r.fixed.lock().unwrap(), ["rooted"]);
    assert_eq!(*r.passwords.lock().unwrap(), [PASSWORD]);
}

fn ask_with(r: &Rig, args: Value, extra: Value) -> Result<Value, notchling_local_api::ApiError> {
    let mut body = json!({"action": "add", "args": args});
    for (k, v) in extra.as_object().unwrap() {
        body[k] = v.clone();
    }
    r.asks.call(&agent_caller(), ops::MACHINES_REQUEST, &body)
}

#[test]
fn machines_add_refuses_a_generic_name() {
    let r = base("generic-name");
    for name in ["server", "host", "new", "machine", "New-Server-2"] {
        let e = r
            .ask("add", json!({"address": "d@newbox.test", "name": name}))
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{name}");
        assert!(e.message.contains("doesn't say which machine"), "{name}");
        assert!(e.message.contains("vps-sing-1"), "{}", e.message);
    }
    assert!(r.rows().is_empty());
    assert!(r.ssh.calls().is_empty());
}

#[test]
fn a_name_after_the_host_passes() {
    let r = base("host-name");
    for (i, name) in ["build-mac", "vps-sing-1", "m1"].into_iter().enumerate() {
        let reply = r
            .ask(
                "add",
                json!({"address": format!("d@box{i}.test"), "name": name}),
            )
            .unwrap();
        assert_eq!(reply["state"], "waiting", "{name}");
    }
}

#[test]
fn the_name_from_user_at_host_is_never_refused() {
    let r = base("parsed-name");
    for address in ["d@server", "d@host.test", "d@new.example"] {
        let reply = r.ask("add", json!({"address": address}));
        assert!(
            reply.as_ref().map_or(true, |v| v["state"] == "waiting"),
            "{address}: {reply:?}"
        );
    }
}

#[test]
fn a_generic_name_never_blocks_a_machine_that_exists_and_a_dry_run_tells_nothing() {
    let r = base("generic-dry");
    let dry = ask_with(
        &r,
        json!({"address": "d@newbox.test", "name": "server"}),
        json!({"dryRun": true}),
    )
    .unwrap();
    assert_eq!(dry["dryRun"], true);
    assert_eq!(dry["would"], "ask");
    assert!(r.rows().is_empty());
    let r = rig(
        "generic-known",
        vec![machine("server", None), machine("mine", None)],
    );
    let known = ask_with(
        &r,
        json!({"address": "d@newbox.test", "name": "server"}),
        json!({}),
    );
    assert!(
        !known
            .as_ref()
            .err()
            .is_some_and(|e| e.message.contains("doesn't say which machine")),
        "{known:?}"
    );
}
