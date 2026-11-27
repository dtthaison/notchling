use notchling_local_api::ops;
use serde_json::{json, Value};

use super::model::ClusterModel;
use super::renders::{replication, status, ME, STANDBY, STUDIO, WORKER};
use super::replication::{is_behind, ReplicationRow, BEHIND_S};

fn with_entry(mut status: Value, entry: Value) -> Value {
    status["replication"] = json!([entry]);
    status
}

fn model_of(status: &Value) -> ClusterModel {
    ClusterModel::from_status(status, ME)
}

#[test]
fn standby_shows_readiness_failures() {
    let status = replication();
    let studio = ReplicationRow::from(&status, STUDIO);
    assert_eq!(
        studio.ready,
        Err("mac-studio has no disk encryption".into())
    );
    assert_eq!(
        studio.not_ready_text().as_deref(),
        Some("Not ready: mac-studio has no disk encryption")
    );
    assert_eq!(studio.set_text(), None);
    assert_eq!(studio.behind_text(), None);

    let mini = ReplicationRow::from(&status, STANDBY);
    assert_eq!(mini.ready, Ok(()));
    assert_eq!(mini.not_ready_text(), None);

    let rows = model_of(&status).rows();
    let shown: Vec<(&str, Option<String>)> = rows
        .iter()
        .filter_map(|r| {
            r.replication
                .as_ref()
                .map(|m| (r.name.as_str(), m.not_ready_text()))
        })
        .collect();
    assert_eq!(
        shown,
        [
            ("mac-mini", None),
            (
                "mac-studio",
                Some("Not ready: mac-studio has no disk encryption".to_owned())
            ),
        ]
    );
    let me = rows.iter().find(|r| r.id == ME).expect("primary");
    assert!(me.replication.is_none());
    let worker = rows.iter().find(|r| r.id == WORKER).expect("worker");
    assert!(worker.replication.is_none());
}

#[test]
fn the_first_failing_check_names_the_reason_when_none_is_given() {
    let mut status = replication();
    for member in status["members"].as_array_mut().expect("members") {
        if member["id"] == STUDIO {
            member["ready"]["reason"] = Value::Null;
            member["ready"]["checks"][0]["text"] = json!("FileVault is off");
        }
    }
    assert_eq!(
        ReplicationRow::from(&status, STUDIO).ready,
        Err("FileVault is off".into())
    );
}

#[test]
fn a_status_without_readiness_reads_as_ready() {
    let mut status = status();
    for member in status["members"].as_array_mut().expect("members") {
        member.as_object_mut().expect("member").remove("ready");
    }
    assert_eq!(ReplicationRow::from(&status, STANDBY).ready, Ok(()));
}

#[test]
fn behind_after_fifteen_minutes() {
    assert_eq!(BEHIND_S, 15 * 60);
    let at = |age: u64, worked: Option<u64>| {
        let entry = json!({"node": STANDBY, "setAgeS": age, "sizeMb": 3, "workedAgeS": worked});
        ReplicationRow::from(&with_entry(status(), entry), STANDBY).behind
    };
    assert!(!at(899, Some(10)));
    assert!(at(900, Some(10)));
    assert!(at(3600, Some(10)));
    assert!(!at(3600, None), "no main session worked, nothing to send");
    assert!(
        !at(3600, Some(7200)),
        "the last work was before the set was made"
    );
    assert!(is_behind(None, Some(5)), "work, and no set ever arrived");
    assert!(!is_behind(None, None));
}

#[test]
fn a_standby_shows_its_last_set_age_and_size() {
    let behind = replication();
    let mini = ReplicationRow::from(&behind, STANDBY);
    assert_eq!(mini.last_set_age_s, Some(1260));
    assert_eq!(mini.size_mb, Some(9));
    assert!(mini.behind);
    assert_eq!(
        mini.set_text().as_deref(),
        Some("Last set 21 minutes ago · 9 MB")
    );
    assert_eq!(
        mini.behind_text().as_deref(),
        Some("Behind: no new set for 21m")
    );

    let fresh = ReplicationRow::from(&status(), STANDBY);
    assert_eq!(
        fresh.set_text().as_deref(),
        Some("Last set 2 minutes ago · 14 MB")
    );
    assert_eq!(fresh.behind_text(), None);

    let ready = ReplicationRow::from(&with_entry(status(), json!({"node": STANDBY})), STANDBY);
    assert_eq!(ready.last_set_age_s, None);
    assert_eq!(ready.size_mb, None);
    assert_eq!(ready.set_text().as_deref(), Some("No set yet"));
    let silent = ReplicationRow::from(&json!({"members": status()["members"]}), STANDBY);
    assert!(!silent.reported);
    assert_eq!(
        silent.set_text(),
        None,
        "a Standby's own status has no set facts"
    );
}

#[test]
fn rows_are_only_for_active_can_be_master_machines_that_are_not_the_primary() {
    let status = replication();
    let nodes: Vec<String> = ReplicationRow::rows(&status)
        .into_iter()
        .map(|r| r.node)
        .collect();
    assert_eq!(nodes, [STANDBY, STUDIO]);
}

#[test]
fn keep_on_machine_only_for_claude_and_codex_on_primary() {
    let primary = model_of(&status());
    let claude = primary
        .keep_offer("j-parser")
        .expect("a main Claude Code session");
    assert_eq!(claude.machine, "MacBook");
    assert!(!claude.kept);
    assert!(primary.keep_offer("codex:c-docs").is_some(), "Codex too");
    assert!(
        primary.keep_offer("agy:a-1").is_none(),
        "Antigravity is never main"
    );
    assert!(
        primary.keep_offer("j-unknown").is_none(),
        "a session the daemon doesn't count as main"
    );
    assert!(
        primary.keep_offer(&format!("{STANDBY}/j-parser")).is_none(),
        "a session on another machine"
    );
    let kept = primary.keep_offer("j-bench").expect("a kept session");
    assert!(kept.kept);

    let ask = primary.keep_toggle("j-parser").expect("turn main off");
    assert_eq!(ask.op, ops::CLUSTER_MAIN_SET);
    assert_eq!(ask.args, json!({"id": "j-parser", "main": false}));
    let back = primary.keep_toggle("j-bench").expect("turn main on again");
    assert_eq!(back.args, json!({"id": "j-bench", "main": true}));
    assert_eq!(primary.keep_toggle("agy:a-1"), None);

    let mut as_standby = status();
    as_standby["me"] = json!(STANDBY);
    let standby = ClusterModel::from_status(&as_standby, STANDBY);
    assert!(standby.keep_offer("j-parser").is_none());
    assert!(ClusterModel::default().keep_offer("j-parser").is_none());
}

#[test]
fn move_to_is_offered_for_a_ready_online_standby_while_the_primary_is_reachable() {
    let model = model_of(&replication());
    assert!(model.offers_move_now(STANDBY));
    assert!(!model.offers_move_now(STUDIO), "not ready");
    assert!(!model.offers_move_now(ME), "already the primary");
    assert!(!model.offers_move_now(WORKER), "a worker can't be master");
    let ask = model.move_home(STANDBY).expect("a request");
    assert_eq!(ask.op, ops::CLUSTER_MOVE_HOME);
    assert_eq!(ask.args, json!({"to": STANDBY}));

    let mut unreachable = replication();
    for member in unreachable["members"].as_array_mut().expect("members") {
        if member["id"] == ME {
            member["online"] = json!(false);
        }
    }
    assert!(!model_of(&unreachable).offers_move_now(STANDBY));

    let mut running = replication();
    running["move"] = json!({"to": STANDBY, "step": "freeze", "moving": [], "staying": []});
    assert!(!model_of(&running).offers_move_now(STANDBY));
}
