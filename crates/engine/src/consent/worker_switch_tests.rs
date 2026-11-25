use super::{member_set_args, worker_switch, WorkerSwitch};
use crate::inbox::{Action, ActionId, Item, Kind, Source, State};
use serde_json::{json, Value};

const NODE: &str = "0123456789abcdef";

fn mirrored(switch: &str) -> Item {
    Item {
        id: format!("{NODE}/consent:c-7:1700"),
        kind: Kind::Consent,
        title: "Claude Code asks for your yes".to_string(),
        detail: String::new(),
        source: Source {
            node: Some(NODE.to_string()),
            session: None,
            job: None,
        },
        created_ms: 1,
        expires_ms: Some(2),
        actions: vec![
            Action {
                id: ActionId::Allow,
                label: "Allow".to_string(),
                primary: true,
            },
            Action {
                id: ActionId::Deny,
                label: "Deny".to_string(),
                primary: false,
            },
        ],
        state: State::Open,
        answer: None,
        answered_by: None,
        seen: false,
        open: None,
        closed_ms: None,
        switch: Some(switch.to_string()),
    }
}

fn status() -> Value {
    json!({
        "members": [
            {"id": "fedcba9876543210", "desired": {"acting": false, "bridge": false, "install": false}},
            {"id": NODE, "desired": {"acting": false, "bridge": true, "install": false}},
        ]
    })
}

#[test]
fn allow_on_a_mirrored_worker_switch_sets_that_switch_on_its_node() {
    let item = mirrored("acting");
    assert_eq!(
        worker_switch(&item),
        Some(WorkerSwitch {
            node: NODE.to_string(),
            switch: "acting".to_string(),
            consent: "c-7".to_string(),
        })
    );
    assert_eq!(
        member_set_args(&item, ActionId::Allow, &status()),
        Some(json!({
            "node": NODE,
            "desired": {"acting": true, "bridge": true, "install": false},
            "consentId": "c-7",
        }))
    );
    assert_eq!(
        member_set_args(&mirrored("install"), ActionId::Allow, &status()).unwrap()["desired"],
        json!({"acting": false, "bridge": true, "install": true})
    );
}

#[test]
fn deny_and_other_actions_set_nothing() {
    let item = mirrored("bridge");
    for action in [ActionId::Deny, ActionId::Open, ActionId::Cancel] {
        assert_eq!(
            member_set_args(&item, action, &status()),
            None,
            "{action:?}"
        );
    }
}

#[test]
fn only_mirrored_consents_for_cluster_switches_on_a_known_member_qualify() {
    for name in ["vault", "accounts", "cluster", ""] {
        assert_eq!(
            member_set_args(&mirrored(name), ActionId::Allow, &status()),
            None
        );
    }
    let mut local = mirrored("acting");
    local.open = Some(json!({"consent": "c-7"}));
    assert_eq!(worker_switch(&local), None);
    let mut bare = mirrored("acting");
    bare.id = "consent:c-7:1700".to_string();
    assert_eq!(worker_switch(&bare), None);
    let mut notice = mirrored("acting");
    notice.kind = Kind::Notice;
    assert_eq!(worker_switch(&notice), None);
    let mut other = mirrored("acting");
    other.id = format!("{NODE}/ask:q-1");
    assert_eq!(worker_switch(&other), None);
    let mut none = mirrored("acting");
    none.switch = None;
    assert_eq!(worker_switch(&none), None);
    assert_eq!(
        member_set_args(
            &mirrored("acting"),
            ActionId::Allow,
            &json!({"members": []})
        ),
        None
    );
}
