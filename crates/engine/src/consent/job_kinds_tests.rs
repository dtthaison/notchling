use super::store_tests::new;
use super::*;
use crate::inbox::ActionId;
use serde_json::json;

pub(super) fn row_of(kind: &str, answer_on: AnswerOn, change: serde_json::Value) -> Consent {
    let store = ConsentStore::in_memory_for_tests();
    let mut asked = new("s1", "job-1");
    asked.kind = kind.to_string();
    asked.answer_on = answer_on;
    asked.change = change;
    store.insert(&asked, 1_000).unwrap().row().clone()
}

#[test]
fn the_job_kinds_are_known_with_their_titles() {
    for (name, title) in [
        (DISPATCH, "Run a job on another machine"),
        (JOB, "Read or return a job"),
        (JOB_STOP, "Stop a job"),
    ] {
        assert_eq!(spec(name).expect(name).title, title);
    }
}

#[test]
fn dispatch_and_job_are_for_the_mac_and_a_stop_may_come_from_a_phone() {
    assert_eq!(spec(DISPATCH).unwrap().answer_on, AnswerOn::Mac);
    assert_eq!(spec(JOB).unwrap().answer_on, AnswerOn::Mac);
    assert_eq!(spec(JOB_STOP).unwrap().answer_on, AnswerOn::MacOrPhone);
    assert_eq!(spec(JOB_STOP).unwrap().result, Outcome::None);
    assert_eq!(spec(DISPATCH).unwrap().result, Outcome::Stored);
}

#[test]
fn each_job_action_has_its_title_naming_the_machine() {
    for (kind, action, words) in [
        (DISPATCH, "dispatch", "asks to run a job on build"),
        (JOB, "tail", "asks to read a job on build"),
        (
            JOB,
            "return",
            "asks to bring back the files of a job on build",
        ),
        (JOB_STOP, "stop", "asks to stop a job on build"),
    ] {
        let row = row_of(
            kind,
            AnswerOn::Mac,
            json!({"action": action, "machine": "build"}),
        );
        let title = title_of(&row);
        assert_eq!(title, format!("Claude Code {words}"), "{action}");
    }
}

#[test]
fn a_job_without_a_machine_name_says_another_machine() {
    let row = row_of(DISPATCH, AnswerOn::Mac, json!({"action": "dispatch"}));
    assert_eq!(
        title_of(&row),
        "Claude Code asks to run a job on another machine"
    );
}

#[test]
fn a_stop_says_it_only_stops_and_the_others_send_you_to_the_card() {
    let stop = row_of(
        JOB_STOP,
        AnswerOn::MacOrPhone,
        json!({"action": "stop", "machine": "build"}),
    );
    assert!(detail_of(&stop).starts_with("It only stops the job."));
    for (kind, action) in [(DISPATCH, "dispatch"), (JOB, "tail"), (JOB, "return")] {
        let row = row_of(
            kind,
            AnswerOn::Mac,
            json!({"action": action, "machine": "m"}),
        );
        assert_eq!(
            detail_of(&row),
            "Open the card on your Mac to read what it would do."
        );
    }
}

#[test]
fn a_mac_only_job_card_offers_open_on_mac_and_a_stop_offers_allow() {
    let dispatch = row_of(
        DISPATCH,
        AnswerOn::Mac,
        json!({"action": "dispatch", "machine": "build"}),
    );
    let ids: Vec<ActionId> = item(&dispatch, 5).actions.iter().map(|a| a.id).collect();
    assert_eq!(ids, vec![ActionId::Deny, ActionId::Open]);
    let stop = row_of(
        JOB_STOP,
        AnswerOn::MacOrPhone,
        json!({"action": "stop", "machine": "build"}),
    );
    let ids: Vec<ActionId> = item(&stop, 5).actions.iter().map(|a| a.id).collect();
    assert_eq!(ids, vec![ActionId::Allow, ActionId::Deny]);
}

#[test]
fn a_long_machine_name_cuts_the_title_to_the_limit() {
    let row = row_of(
        DISPATCH,
        AnswerOn::Mac,
        json!({"action": "dispatch", "machine": "m".repeat(200)}),
    );
    assert_eq!(title_of(&row).chars().count(), TITLE_CHARS);
}

#[test]
fn the_cluster_kind_is_for_the_mac_and_the_app_applies_it() {
    let kind = spec(CLUSTER).unwrap();
    assert_eq!(kind.title, "Change the cluster");
    assert_eq!(kind.answer_on, AnswerOn::Mac);
    assert_eq!(kind.applied_by, AppliedBy::App);
}

#[test]
fn each_cluster_action_has_a_title_that_holds_no_agent_text() {
    for (action, words) in [
        ("create", "asks to create a cluster"),
        ("token", "asks for a join token"),
        ("invite", "asks to invite a machine to the cluster"),
        ("member", "asks to change build in the cluster"),
        ("remove", "asks to remove build from the cluster"),
        ("allow", "asks to change the secrets build may use"),
        ("main", "asks to change a main session of the cluster"),
        ("failover", "asks to change the cluster's failover"),
        ("move_home", "asks to make build the cluster's primary"),
        ("move_now", "asks to finish moving the cluster's primary"),
    ] {
        let row = row_of(
            CLUSTER,
            AnswerOn::Mac,
            json!({"action": action, "machine": "build", "name": "Evil Name", "cluster": "Evil"}),
        );
        let title = title_of(&row);
        assert_eq!(title, format!("Claude Code {words}"), "{action}");
        assert!(!title.contains("Evil"));
        assert_eq!(
            detail_of(&row),
            "Open the card on your Mac to read what it would do."
        );
    }
}

#[test]
fn a_cluster_item_offers_deny_and_open_on_the_mac() {
    let row = row_of(CLUSTER, AnswerOn::Mac, json!({"action": "create"}));
    let shown = item(&row, 5);
    let ids: Vec<ActionId> = shown.actions.iter().map(|a| a.id).collect();
    assert_eq!(ids, vec![ActionId::Deny, ActionId::Open]);
}

#[test]
fn the_account_kind_is_for_the_mac_the_app_applies_it_and_keeps_a_result() {
    let kind = spec(ACCOUNT).unwrap();
    assert_eq!(kind.title, "Change accounts");
    assert_eq!(kind.answer_on, AnswerOn::Mac);
    assert_eq!(kind.applied_by, AppliedBy::App);
    assert_eq!(kind.result, Outcome::Stored);
    assert!(!kind.parts);
    let names: Vec<&str> = KINDS.iter().map(|k| k.name).collect();
    assert_eq!(
        names,
        vec![
            SETTING,
            SECRET_REQUEST,
            SWITCH,
            SESSION,
            SESSION_STOP,
            DISPATCH,
            JOB,
            JOB_STOP,
            CLUSTER,
            ACCOUNT,
            MACHINE,
            UPDATE,
            INSTALL,
            COST_PROFILE,
            MODEL
        ]
    );
}

#[test]
fn account_titles_name_the_tool_and_never_the_email() {
    for (tool, action, words) in [
        (
            "claude",
            "switch",
            "asks to switch your Claude Code account",
        ),
        ("codex", "switch", "asks to switch your Codex account"),
        (
            "antigravity",
            "switch",
            "asks to switch your Antigravity account",
        ),
        ("codex", "add", "asks to add a Codex account"),
        ("claude", "remove", "asks to remove a Claude Code account"),
        (
            "claude",
            "move_all",
            "asks to move sessions to the main login",
        ),
        ("claude", "move", "asks to move a session to another login"),
        ("claude", "lane_create", "asks to open a lane"),
        ("claude", "lane_remove", "asks to close a lane"),
        (
            "claude",
            "lane_switch",
            "asks to change the account of a lane",
        ),
        ("claude", "other", "asks for your yes"),
    ] {
        let row = row_of(
            ACCOUNT,
            AnswerOn::Mac,
            json!({"action": action, "tool": tool, "account": "acct-2222222222222222",
                   "email": "bob@work.com", "name": "Evil"}),
        );
        let title = title_of(&row);
        assert_eq!(title, format!("Claude Code {words}"), "{action}");
        assert!(!title.contains("bob") && !title.contains("acct-") && !title.contains("Evil"));
        assert_eq!(
            detail_of(&row),
            "Open the card on your Mac to read what it would do."
        );
        let ids: Vec<ActionId> = item(&row, 5).actions.iter().map(|a| a.id).collect();
        assert_eq!(ids, vec![ActionId::Deny, ActionId::Open]);
    }
}
