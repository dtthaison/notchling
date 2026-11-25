use serde_json::{json, Value};

use super::*;

const NOW: i64 = 1_794_096_000_000;

fn env() -> Env {
    Env {
        now_ms: NOW,
        vault_on: true,
        shown_ms: 5_000,
        node: None,
        ..Env::default()
    }
}

fn cluster(change: Value, facts: Value) -> Value {
    json!({
        "id": "c9", "kind": "cluster", "target": "invite:n-spare", "state": "waiting",
        "change": change, "facts": facts,
        "tool": "claude", "verified": true, "session": "0b1c2d3e4f5a",
        "expiresMs": NOW + 14 * 60_000, "appliedBy": "app", "answerOn": "mac"
    })
}

fn apply_of(op: &str, args: Value) -> Value {
    json!({"action": "invite", "organization": "Personal", "apply": {"op": op, "args": args}})
}

#[test]
fn an_invite_says_the_cluster_the_role_and_the_machine() {
    let view = cluster(
        apply_of("cluster.invite", json!({"node": "n-spare"})),
        json!([
            {"line": "consent-cluster", "args": {"cluster": "Home"}},
            {"line": "consent-cluster-invite",
             "args": {"name": "spare-box", "node": "n-spare", "role": "worker", "acting": "off"}},
            {"line": "consent-cluster-machine",
             "args": {"machine": "spare-box", "address": "spare.ts.net", "organization": "Personal"}},
        ]),
    );
    let card = ConsentCard::build(&view, &env()).expect("card");
    assert_eq!(
        card.facts,
        vec![
            t::consent::card_fact_cluster("Home"),
            t::consent::card_fact_cluster_invite(
                "spare-box",
                "n-spare",
                t::consent::card_word_worker(),
                t::consent::card_word_off()
            ),
            t::consent::card_fact_cluster_machine("spare-box", "spare.ts.net", "Personal"),
        ]
    );
    assert!(card.facts[2].contains("spare.ts.net"));
}

#[test]
fn a_machine_without_an_address_is_said_without_one() {
    let view = cluster(
        apply_of("cluster.remove", json!({"node": "n-build"})),
        json!([{"line": "consent-cluster-machine",
                "args": {"machine": "build", "address": "", "organization": "Personal"}}]),
    );
    let card = ConsentCard::build(&view, &env()).expect("card");
    assert_eq!(
        card.facts,
        vec![t::consent::card_fact_cluster_machine_no_address(
            "build", "Personal"
        )]
    );
}

#[test]
fn a_change_says_before_and_after_in_words() {
    let cases = [
        (
            "consent-cluster-role",
            json!({"before": "worker", "after": "master"}),
            t::consent::card_fact_cluster_role(
                t::consent::card_word_worker(),
                t::consent::card_word_master(),
            ),
        ),
        (
            "consent-cluster-delegation",
            json!({"before": "off", "after": "on"}),
            t::consent::card_fact_cluster_delegation(
                t::consent::card_word_off(),
                t::consent::card_word_on(),
            ),
        ),
        (
            "consent-cluster-failover",
            json!({"before": "off", "after": "30m"}),
            t::consent::card_fact_cluster_failover(t::consent::card_word_off(), "30m"),
        ),
        (
            "consent-cluster-main",
            json!({"session": "0b1c2d3e4f5a6b7c", "before": "off", "after": "on"}),
            t::consent::card_fact_cluster_main(
                "0b1c2d3e",
                t::consent::card_word_off(),
                t::consent::card_word_on(),
            ),
        ),
    ];
    for (line, args, want) in cases {
        let view = cluster(
            apply_of("cluster.member.set", json!({})),
            json!([{"line": line, "args": args}]),
        );
        let card = ConsentCard::build(&view, &env()).expect("card");
        assert_eq!(card.facts, vec![want], "{line}");
    }
}

#[test]
fn the_other_lines_name_what_they_change() {
    let cases = [
        (
            "consent-cluster-create",
            json!({"name": "Home", "organization": "Personal"}),
            t::consent::card_fact_cluster_create("Home", "Personal"),
        ),
        (
            "consent-cluster-token",
            json!({}),
            t::consent::card_fact_cluster_token().to_owned(),
        ),
        (
            "consent-cluster-allow-secret",
            json!({"name": "deploy-token"}),
            t::consent::card_fact_cluster_allow_secret("deploy-token"),
        ),
        (
            "consent-cluster-allow-none",
            json!({}),
            t::consent::card_fact_cluster_allow_none().to_owned(),
        ),
        (
            "consent-cluster-remove",
            json!({"machine": "build"}),
            t::consent::card_fact_cluster_remove("build"),
        ),
        (
            "consent-cluster-move-home",
            json!({"machine": "build"}),
            t::consent::card_fact_cluster_move_home("build"),
        ),
        (
            "consent-cluster-move-now",
            json!({}),
            t::consent::card_fact_cluster_move_now().to_owned(),
        ),
    ];
    for (line, args, want) in cases {
        let view = cluster(
            apply_of("cluster.create", json!({})),
            json!([{"line": line, "args": args}]),
        );
        let card = ConsentCard::build(&view, &env()).expect("card");
        assert_eq!(card.facts, vec![want], "{line}");
    }
}

#[test]
fn a_line_missing_its_words_is_left_out() {
    let view = cluster(
        apply_of("cluster.remove", json!({})),
        json!([
            {"line": "consent-cluster-remove", "args": {}},
            {"line": "consent-cluster-role", "args": {"before": "worker"}},
        ]),
    );
    assert!(ConsentCard::build(&view, &env())
        .expect("card")
        .facts
        .is_empty());
}

#[test]
fn allow_calls_the_op_the_request_names() {
    let args =
        json!({"node": "n-spare", "name": "spare-box", "canBeMaster": false, "acting": false});
    let view = cluster(apply_of("cluster.invite", args.clone()), json!([]));
    let card = ConsentCard::build(&view, &env()).expect("card");
    assert_eq!(
        card.cluster_apply,
        Some(ClusterApply {
            op: "cluster.invite".into(),
            args
        })
    );
    assert!(card.app_applied);
    assert_eq!(card.switch, None);
}

#[test]
fn only_a_cluster_request_carries_an_apply() {
    let mut view = cluster(apply_of("cluster.invite", json!({})), json!([]));
    view["kind"] = json!("setting");
    assert_eq!(
        ConsentCard::build(&view, &env())
            .expect("card")
            .cluster_apply,
        None
    );
    let mut bare = cluster(json!({"action": "invite"}), json!([]));
    assert_eq!(
        ConsentCard::build(&bare, &env())
            .expect("card")
            .cluster_apply,
        None
    );
    bare["change"] = json!({"apply": {"op": "cluster.invite", "args": "x"}});
    assert_eq!(
        ConsentCard::build(&bare, &env())
            .expect("card")
            .cluster_apply,
        None
    );
}

#[test]
fn the_card_shows_the_organization_of_the_machine() {
    let view = cluster(apply_of("cluster.invite", json!({})), json!([]));
    let card = ConsentCard::build(&view, &env()).expect("card");
    assert!(card
        .details
        .iter()
        .any(|d| d.label == t::consent::card_label_organization() && d.value == "Personal"));
}
