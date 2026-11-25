use std::collections::HashSet;

use serde_json::{json, Value};

use super::*;
use crate::inbox::consent_family::{family_apply, kept_ids, part_also, toggled, Admin, Family};
use crate::inbox::family_fixtures::*;
use crate::inbox::view_family::{only_entry, FamilyState};
use notchling_engine::vault::entry::{Kind, VaultEntry};

fn card(view: &Value) -> ConsentCard {
    ConsentCard::build(view, &env()).expect("card")
}

#[test]
fn an_update_card_shows_the_daemons_fact_lines() {
    let card = card(&update_view());
    assert_eq!(
        card.facts,
        vec![
            "Update Codex (Homebrew) from 0.9.1 to 0.9.4".to_owned(),
            "Sessions running it keep the old version until they restart.".to_owned(),
        ]
    );
    assert!(card.allow.enabled() && card.parts.is_empty());
}

#[test]
fn the_other_update_lines_read_plainly() {
    let line = |name: &str, args: Value| fact_line_of(name, &args);
    assert_eq!(
        line(
            "consent-update-tool-latest",
            json!({"tool": "Codex", "method": "npm", "from": "1.0"})
        ),
        "Update Codex (npm) from 1.0 to its newest version"
    );
    assert_eq!(
        line(
            "consent-update-back",
            json!({"tool": "Codex", "from": "1.2", "to": "1.1"})
        ),
        "Go back Codex from 1.2 to 1.1"
    );
}

fn fact_line_of(name: &str, args: &Value) -> String {
    super::consent_family::fact_line(name, args).expect("a fact")
}

#[test]
fn a_machine_card_shows_the_machine_its_steps_and_the_access_it_needs() {
    let card = card(&machine_view(true));
    assert_eq!(
        card.facts,
        vec![
            "Machine m1, reached as maker@m1.ts.net, in Personal. Addresses: m1.ts.net, 10.0.0.4. ssh key file: id_ed25519"
                .to_owned(),
            "Install the tool: brew install codex".to_owned(),
            "Administrator access: the administrator password of the machine".to_owned(),
        ]
    );
    assert!(card.details.iter().any(|d| d.value == "Personal"));
}

#[test]
fn an_add_card_names_the_machine_and_leaves_out_an_empty_key() {
    let card = card(&machine_add_view());
    assert_eq!(
        card.facts,
        vec![
            "Machine m1, reached as maker@m1.ts.net, in Personal. Addresses: m1.ts.net".to_owned(),
            "Notchling adds m1 to its machines.".to_owned(),
        ]
    );
    assert!(card.allow.enabled());
}

#[test]
fn the_other_machine_lines_read_plainly() {
    let line = fact_line_of;
    assert_eq!(
        line("consent-machine-import", &json!({"machines": "m1, m2"})),
        "Notchling adds these machines: m1, m2"
    );
    assert_eq!(
        line(
            "consent-machine-addresses",
            &json!({"before": "a", "after": "a, b"})
        ),
        "Addresses change from a to a, b"
    );
    assert_eq!(
        line("consent-machine-remove", &json!({"machine": "m1"})),
        "Notchling forgets m1. Nothing on the machine changes."
    );
    assert_eq!(
        line("consent-machine-admin", &json!({"need": "root"})),
        "Administrator access: steps run as root"
    );
}

#[test]
fn a_network_card_lists_what_drops() {
    let card = card(&network_view());
    assert_eq!(
        card.facts,
        vec![
            "Switch this Mac's Tailscale network from h***@gmail.com to w***@gmail.com".to_owned(),
            "2 machines will lose their connection".to_owned(),
            "1 cluster member loses this Mac. Failover is off.".to_owned(),
            "Your phone may lose the bridge if it uses Tailscale.".to_owned(),
        ]
    );
    let apply = card.family_apply.expect("apply");
    assert_eq!(apply.op, "tailscale.switch");
    assert_eq!(
        fact_line_of("consent-network-drops", &json!({"count": 1})),
        "1 machine will lose its connection"
    );
}

#[test]
fn an_install_card_has_one_ticked_row_for_each_file() {
    let card = card(&install_view());
    assert_eq!(card.facts, vec![t::consent::install_backup().to_owned()]);
    let ids: Vec<&str> = card.parts.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["hooks", "skill:claude:notchling-vault"]);
    assert!(card.parts.iter().all(|p| p.kept));
    assert!(card.allow.enabled());
    assert_eq!(card.parts[0].added, 4);
    assert_eq!(card.parts[1].change, "create");
}

#[test]
fn a_file_row_lists_the_other_files_that_change_with_it() {
    let mut view = install_view();
    view["parts"][0]["also"] = json!([
        {"id": "agy-hooks", "path": "/home/u/.gemini/hooks.json", "before": null},
        {"id": "no-path"},
    ]);
    let card = card(&view);
    assert_eq!(card.parts[0].also, ["/home/u/.gemini/hooks.json"]);
    assert!(card.parts[1].also.is_empty());
    assert_eq!(
        part_also(&card.parts[0].also[0]),
        t::consent::install_part_also("/home/u/.gemini/hooks.json")
    );
}

#[test]
fn a_cost_card_has_file_rows_and_a_profile_row_with_its_fields() {
    let card = card(&cost_view());
    assert_eq!(card.parts.len(), 2);
    assert_eq!(card.parts[1].fields.len(), 1);
    assert_eq!(
        super::consent_family::part_field(&card.parts[1].fields[0]),
        "preset: Balanced to Lean"
    );
}

#[test]
fn unticking_a_row_leaves_it_out_of_the_kept_ids() {
    let mut unticked = HashSet::new();
    toggled(&mut unticked, "hooks");
    let card = ConsentCard::build(
        &install_view(),
        &Env {
            unticked: unticked.clone(),
            ..env()
        },
    )
    .expect("card");
    assert!(!card.parts[0].kept && card.parts[1].kept);
    assert_eq!(
        kept_ids(&card.parts),
        json!(["skill:claude:notchling-vault"])
    );
    assert!(card.allow.enabled());
    toggled(&mut unticked, "hooks");
    assert!(unticked.is_empty());
}

#[test]
fn allow_is_off_with_nothing_kept() {
    let all: HashSet<String> = ["hooks", "skill:claude:notchling-vault"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let card = ConsentCard::build(
        &install_view(),
        &Env {
            unticked: all,
            ..env()
        },
    )
    .expect("card");
    assert!(!card.allow.enabled());
    assert_eq!(
        card.blocked.as_deref(),
        Some(t::consent::install_none_kept())
    );
    assert!(card.answerable);
}

#[test]
fn a_machine_card_that_needs_admin_keeps_allow_off_until_one_is_given() {
    let waiting = card(&machine_view(true));
    assert!(!waiting.allow.enabled());
    assert_eq!(
        waiting.blocked.as_deref(),
        Some(t::consent::machine_password_needed())
    );
    let ready = ConsentCard::build(
        &machine_view(true),
        &Env {
            admin_ready: true,
            ..env()
        },
    )
    .expect("card");
    assert!(ready.allow.enabled() && ready.blocked.is_none());
    assert!(card(&machine_view(false)).allow.enabled());
}

#[test]
fn a_machine_card_whose_admin_is_refused_keeps_allow_off() {
    let mut view = machine_view(false);
    view["change"]["admin"] = json!("refused");
    for env in [
        env(),
        Env {
            admin_ready: true,
            ..env()
        },
    ] {
        let card = ConsentCard::build(&view, &env).expect("card");
        assert!(!card.allow.enabled());
        assert_eq!(
            card.blocked.as_deref(),
            Some(t::consent::machine_admin_refused())
        );
    }
}

#[test]
fn the_set_up_and_cost_lines_read_plainly() {
    let line = fact_line_of;
    assert_eq!(
        line("consent-install-files", &json!({"count": 2})),
        "Set Up writes 2 files for Notchling"
    );
    assert_eq!(
        line("consent-install-sidecar", &json!({})),
        "Turn on the Antigravity sidecar in Antigravity's config.json"
    );
    assert_eq!(
        line("consent-cost-apply", &json!({"count": 1})),
        "The Cost Profile writes 1 file"
    );
    assert_eq!(
        line("consent-cost-undo", &json!({"tool": "agy", "count": 3})),
        format!(
            "Put back 3 files the Cost Profile changed for {}",
            t::consent::card_tool_antigravity()
        )
    );
    assert_eq!(
        line("consent-cost-profile", &json!({"count": 2})),
        "Change 2 fields of the Cost Profile"
    );
}

#[test]
fn the_admin_password_goes_only_into_machines_apply_and_never_into_a_debug_line() {
    let machine = card(&machine_view(true)).family_apply.expect("apply");
    let typed = Admin::Password(notchling_engine::accounts::Secret::new("hunter2"));
    let sent = machine.request("c20", &[], Some(&typed));
    assert_eq!(
        sent,
        json!({"machine": "m1", "consentId": "c20", "admin": {"password": "hunter2"}})
    );
    assert!(!format!("{typed:?}").contains("hunter2"));
    let vault = machine.request("c20", &[], Some(&Admin::Vault("m1-admin".into())));
    assert_eq!(vault["admin"], json!({"vault": "m1-admin"}));
    let update = card(&update_view()).family_apply.expect("apply");
    assert!(update
        .request("c20", &[], Some(&typed))
        .get("admin")
        .is_none());
    assert!(!format!("{:?}", card(&machine_view(true))).contains("hunter2"));
}

#[test]
fn another_kind_has_no_family_apply() {
    let view = json!({"id": "c1", "kind": "setting", "state": "waiting", "expiresMs": NOW + 1000,
                      "change": {"after": "x", "apply": {"op": "x", "args": {}}}});
    let card = ConsentCard::build(&view, &env()).expect("card");
    assert!(card.family_apply.is_none() && card.parts.is_empty());
}

#[test]
fn a_machine_card_carries_the_admin_entry_the_daemon_named() {
    let mut view = machine_view(true);
    assert_eq!(card(&view).family_apply.expect("apply").admin_entry, None);
    view["change"]["adminEntry"] = json!("m1-sudo");
    assert_eq!(
        card(&view).family_apply.expect("apply").admin_entry,
        Some("m1-sudo".to_owned())
    );
    view["kind"] = json!("update");
    view["change"]["adminEntry"] = json!("m1-sudo");
    let other = family_apply(&view).expect("apply");
    assert_eq!(other.admin_entry, None);
}

#[test]
fn the_network_card_lists_the_machines_that_drop_for_each_organization() {
    let groups = json!([
        {"organization": "Personal", "machines": ["m1", "m2"]},
        {"organization": "Work", "machines": ["w1"]}
    ]);
    assert_eq!(
        fact_line_of(
            "consent-network-drops",
            &json!({"count": 3, "groups": groups})
        ),
        "3 machines will lose their connection. Personal: m1, m2. Work: w1"
    );
    assert_eq!(
        fact_line_of(
            "consent-network-drops",
            &json!({"count": 2, "groups": [{"organization": "Personal", "count": 2}]})
        ),
        "2 machines will lose their connection"
    );
}

#[test]
fn each_card_keeps_its_own_vault_pick_and_choice() {
    let mut family = FamilyState::default();
    assert!(family.toggle_pick("a"));
    assert!(family.pick_of("a").is_open());
    assert!(!family.pick_of("b").is_open());
    family.choose("a", "a-sudo");
    assert!(!family.pick_of("a").is_open());
    assert_eq!(family.chosen("a"), Some("a-sudo"));
    assert_eq!(family.chosen("b"), None);
    family.choose("b", "b-sudo");
    family.clear_choice("a");
    assert_eq!(family.chosen("a"), None);
    assert_eq!(family.chosen("b"), Some("b-sudo"));
    assert!(family.toggle_pick("b"));
    family.loaded(
        "b",
        Some("B-SUDO"),
        Ok(vec![VaultEntry::new("b-sudo", Kind::Login)]),
    );
    assert_eq!(family.pick_of("b").rows(Some("b-sudo")).len(), 1);
    assert!(family.pick_of("a").rows(None).is_empty());
}

#[test]
fn the_picker_lists_only_the_machines_saved_entry_or_nothing() {
    let all = || {
        Ok(vec![
            VaultEntry::new("m1-sudo", Kind::Login),
            VaultEntry::new("other", Kind::Token),
        ])
    };
    let names = |r: Result<Vec<VaultEntry>, String>| -> Vec<String> {
        r.unwrap().into_iter().map(|e| e.name).collect()
    };
    assert_eq!(names(only_entry(all(), Some("M1-SUDO"))), ["m1-sudo"]);
    assert_eq!(names(only_entry(all(), Some(" m1-Sudo "))), ["m1-sudo"]);
    assert!(names(only_entry(all(), Some("missing"))).is_empty());
    assert!(names(only_entry(all(), None)).is_empty());
    assert!(only_entry(Err("locked".into()), Some("m1-sudo")).is_err());
}

#[test]
fn a_model_card_names_the_model_and_applies_embed_fetch_with_its_id() {
    let view = model_view();
    let card = card(&view);
    let apply = card.family_apply.expect("apply");
    assert_eq!(apply.op, "embed.fetch");
    assert_eq!(apply.family, Family::Model);
    assert!(!apply.family.has_parts());
    let sent = apply.request("c20", &[], None);
    assert_eq!(
        sent,
        json!({"model": "granite-97m-r2@835ad14/p1", "consentId": "c20"})
    );
    assert_eq!(
        fact_line_of(
            "consent-model-download",
            &json!({"model": "Standard", "download": "195", "kept": "195"})
        ),
        "Download Standard: 195 MB to fetch, 195 MB kept on this Mac"
    );
}
