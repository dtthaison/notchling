use super::*;
use serde_json::json;

fn worker(linked: bool) -> Place {
    Place::Worker {
        name: "m9".to_string(),
        linked,
    }
}

#[test]
fn a_mac_or_a_worker_with_its_app_is_never_refused() {
    for name in crate::switch_asks::NAMES {
        assert!(refusal(&Place::Mac, false, name).is_none());
        assert!(refusal(&worker(false), true, name).is_none());
        assert!(!forwarded(&Place::Mac, false, name));
        assert!(!forwarded(&worker(true), true, name));
    }
}

#[test]
fn a_linked_worker_without_an_app_forwards_only_what_the_cluster_sets() {
    for name in ["acting", "bridge", "install"] {
        assert!(refusal(&worker(true), false, name).is_none());
        assert!(forwarded(&worker(true), false, name));
    }
    for name in ["accounts", "vault", "cluster"] {
        let refused = refusal(&worker(true), false, name).expect(name);
        assert_eq!(refused.code, ErrorCode::Forbidden);
        assert_eq!(refused.data.unwrap()["reason"], json!("no_app"));
        assert!(!forwarded(&worker(true), false, name));
    }
}

#[test]
fn an_unlinked_worker_without_an_app_refuses_every_switch_with_no_app() {
    for name in crate::switch_asks::NAMES {
        let refused = refusal(&worker(false), false, name).expect(name);
        assert_eq!(refused.message, "Open Notchling on m9 to allow this.");
        assert_eq!(refused.data.unwrap()["reason"], json!("no_app"));
    }
    assert_eq!(
        crate::contract::exit_of_reason("no_app"),
        Some(crate::contract::REFUSED)
    );
}

#[test]
fn the_link_is_up_only_while_the_home_is_online() {
    let status = |online: bool| {
        json!({"cluster": {"home": "h1"}, "members": [
            {"id": "w9", "online": true},
            {"id": "h1", "online": online},
        ]})
    };
    assert!(home_online(&status(true)));
    assert!(!home_online(&status(false)));
    assert!(!home_online(
        &json!({"members": [{"id": "h1", "online": true}]})
    ));
}
