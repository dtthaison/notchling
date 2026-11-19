use notchling_engine::machines::addresses::AddressKind;
use notchling_engine::machines::registry::Machine;
use notchling_engine::machines::tailscale::{parse_networks, parse_status};

use super::add_rig_tests::Rig;
use notchling_engine::machines::ssh_key::{KeyIssue, KeyStatus};
use notchling_engine::machines::ssh_test::SshTest;

use super::from_tailscale::{import_machines, FromTailscale, Read, TestState};

const LIST: &str = "ID    Tailnet              Account\na1b2  person@example.com   person@example.com*\nc3d4  work.example         work@example.com\n";

const STATUS: &str = r#"{
  "BackendState": "Running",
  "Self": {"HostName": "studio-mac", "DNSName": "studio-mac.tail0a1b2.ts.net.", "OS": "macOS", "UserID": 1001, "TailscaleIPs": ["100.64.0.10"], "Online": true},
  "CurrentTailnet": {"Name": "person@example.com", "MagicDNSSuffix": "tail0a1b2.ts.net", "MagicDNSEnabled": true},
  "Peer": {
    "nodekey:aa": {"ID": "n1", "HostName": "m1", "DNSName": "m1.tail0a1b2.ts.net.", "OS": "macOS", "UserID": 1001, "TailscaleIPs": ["100.64.0.11"], "Online": true},
    "nodekey:bb": {"ID": "n2", "HostName": "oracle-sg", "DNSName": "oracle-sg.tail0a1b2.ts.net.", "OS": "linux", "UserID": 1003, "TailscaleIPs": ["100.64.0.12"], "Online": false, "Tags": ["tag:server"]},
    "nodekey:cc": {"ID": "n3", "HostName": "winbox", "DNSName": "winbox.tail0a1b2.ts.net.", "OS": "windows", "UserID": 1001, "TailscaleIPs": ["100.64.0.14"], "Online": true}
  },
  "User": {
    "1001": {"ID": 1001, "LoginName": "person@example.com", "DisplayName": "Person"},
    "1003": {"ID": 1003, "LoginName": "tagged-devices", "DisplayName": "Tagged"}
  }
}"#;

fn ready(existing: Vec<Machine>) -> FromTailscale {
    let mut model = FromTailscale::new("me", existing);
    model.loaded(parse_status(STATUS), parse_networks(LIST));
    model
}

#[test]
fn the_list_is_the_active_network_and_names_it() {
    let model = ready(vec![]);
    assert_eq!(model.read, Read::Ready);
    assert_eq!(model.network_name(), "person@example.com");
    let names: Vec<String> = model.rows().into_iter().map(|r| r.title).collect();
    assert_eq!(names, ["m1", "oracle-sg", "winbox"]);
    let rows = model.rows();
    assert!(rows[0].online && !rows[1].online);
    assert!(rows[0].detail.contains("100.64.0.11"));
    assert!(rows[1].owner.contains("tag:server"));
}

#[test]
fn the_other_network_is_named_and_listing_both_waits_for_api_keys() {
    let model = ready(vec![]);
    let note = model.other_networks_note().unwrap();
    assert_eq!(
        note,
        "Switch Tailscale to work.example to list its machines; API keys for listing both come later."
    );
    let mut alone = FromTailscale::new("me", vec![]);
    alone.loaded(
        parse_status(STATUS),
        parse_networks("ID Tailnet Account\na1b2 person@example.com person@example.com*\n"),
    );
    assert!(alone.other_networks_note().is_none());
}

#[test]
fn machines_already_in_machines_are_hidden_and_counted() {
    let model = ready(vec![Machine::new("home", "m1.tail0a1b2.ts.net", "me")]);
    let names: Vec<String> = model.rows().into_iter().map(|r| r.title).collect();
    assert_eq!(names, ["oracle-sg", "winbox"]);
    assert_eq!(model.hidden_count(), 1);
}

#[test]
fn the_filter_works_on_text_system_and_online() {
    let mut model = ready(vec![]);
    model.filter.text = "oracle".into();
    assert_eq!(model.rows().len(), 1);
    model.filter.text.clear();
    model.filter.os = Some("linux".into());
    assert_eq!(model.rows().len(), 1);
    model.filter.os = None;
    model.filter.online_only = true;
    assert_eq!(model.rows().len(), 2);
    assert_eq!(model.systems(), ["linux", "macOS", "windows"]);
}

#[test]
fn select_all_takes_what_is_shown_and_again_clears_it() {
    let mut model = ready(vec![]);
    model.filter.online_only = true;
    model.select_all();
    assert_eq!(model.chosen.len(), 2);
    assert!(model.chosen.values().all(|u| u == "me"));
    assert!(model.all_chosen());
    model.select_all();
    assert!(model.chosen.is_empty());
    model.toggle("m1.tail0a1b2.ts.net");
    assert_eq!(model.chosen.len(), 1);
    model.set_user("m1.tail0a1b2.ts.net", " deploy ");
    assert_eq!(model.chosen["m1.tail0a1b2.ts.net"], "deploy");
    assert!(model.can_import());
    model.set_user("m1.tail0a1b2.ts.net", "Bad User");
    assert!(!model.can_import());
}

#[test]
fn a_stopped_tailscale_or_a_failed_read_says_so_and_imports_nothing() {
    let mut off = FromTailscale::new("me", vec![]);
    off.loaded(
        parse_status(r#"{"BackendState": "Stopped", "Peer": {}}"#),
        vec![],
    );
    assert!(matches!(off.read, Read::Failed(_)));
    assert!(!off.can_import());
    let mut failed = FromTailscale::new("me", vec![]);
    failed.loaded(Err("tailscale: not found".into()), vec![]);
    assert_eq!(failed.read, Read::Failed("tailscale: not found".into()));
}

#[test]
fn import_saves_machines_with_tailscale_addresses_and_nothing_else() {
    let rig = Rig::new(vec![]);
    let mut model = ready(vec![]);
    model.toggle("m1.tail0a1b2.ts.net");
    model.toggle("oracle-sg.tail0a1b2.ts.net");
    model.set_user("oracle-sg.tail0a1b2.ts.net", "ubuntu");
    let count = import_machines(&rig.parts(), &model.status, &model.picked(), None).unwrap();
    assert_eq!(count, 2);
    let m1 = rig.saved("m1").expect("saved");
    assert_eq!(m1.user, "me");
    assert!(m1.node.is_none());
    assert!(m1.jobs.is_empty());
    assert_eq!(m1.address_list().len(), 2);
    assert_eq!(m1.address_list()[0].kind, AddressKind::TailscaleName);
    assert_eq!(
        m1.address_list()[0].tailnet.as_deref(),
        Some("person@example.com")
    );
    assert_eq!(m1.address_list()[1].host, "100.64.0.11");
    let oracle = rig.saved("oracle-sg").expect("saved");
    assert_eq!(oracle.user, "ubuntu");
    assert_eq!(rig.fix_calls(), 0);
    assert!(rig.world.fixed().is_empty());
    let again = import_machines(&rig.parts(), &model.status, &model.picked(), None).unwrap();
    assert_eq!(again, 0, "a second import adds nothing");
}

#[test]
fn import_refuses_a_bad_user_and_changes_nothing() {
    let rig = Rig::new(vec![]);
    let mut model = ready(vec![]);
    model.toggle("m1.tail0a1b2.ts.net");
    model.set_user("m1.tail0a1b2.ts.net", "Bad User");
    assert!(import_machines(&rig.parts(), &model.status, &model.picked(), None).is_err());
    assert!(rig.saved("m1").is_none());
}

const ORGS: &str = r#"{"organizations": [
  {"id": "personal", "name": "Personal", "personal": true},
  {"id": "org-acme", "name": "Acme", "personal": false}
], "colors": ["grey"]}"#;

fn orgs(model: &mut FromTailscale) {
    model.set_organizations(Ok(serde_json::from_str(ORGS).unwrap()));
}

#[test]
fn the_picker_defaults_to_personal_and_cycles_through_organizations() {
    let mut model = ready(vec![]);
    assert!(!model.offers_organizations());
    orgs(&mut model);
    assert!(model.offers_organizations());
    assert_eq!(model.organization_name(), "Personal");
    assert_eq!(model.organization_choice(), None);
    model.next_organization();
    assert_eq!(model.organization_name(), "Acme");
    assert_eq!(model.organization_choice().as_deref(), Some("org-acme"));
    model.next_organization();
    assert_eq!(model.organization_choice(), None);
}

#[test]
fn a_failed_organization_read_leaves_personal_and_no_picker() {
    let mut model = ready(vec![]);
    model.set_organizations(Err("daemon is not running".into()));
    assert!(!model.offers_organizations());
    assert_eq!(model.organization_choice(), None);
}

#[test]
fn import_files_the_machines_under_the_chosen_organization() {
    let rig = Rig::new(vec![]);
    let mut model = ready(vec![]);
    orgs(&mut model);
    model.next_organization();
    model.toggle("m1.tail0a1b2.ts.net");
    let org = model.organization_choice();
    import_machines(&rig.parts(), &model.status, &model.picked(), org.as_deref()).unwrap();
    assert_eq!(
        rig.saved("m1").unwrap().organization.as_deref(),
        Some("org-acme")
    );
}

#[test]
fn an_import_that_adds_nothing_says_so() {
    let mut model = ready(vec![]);
    model.imported(0);
    assert_eq!(model.done, Some(0));
}

const M1: &str = "m1.tail0a1b2.ts.net";
const ORACLE: &str = "oracle-sg.tail0a1b2.ts.net";

fn usable() -> KeyStatus {
    KeyStatus {
        issue: None,
        public: false,
    }
}

fn running_round(model: &FromTailscale, host: &str) -> u64 {
    match model.tests.get(host) {
        Some(TestState::Running(round)) => *round,
        other => panic!("{other:?}"),
    }
}

#[test]
fn only_a_chosen_row_can_be_tested_and_it_waits_for_a_lane() {
    let mut model = ready(vec![]);
    assert!(!model.request_test(M1));
    model.toggle(M1);
    assert!(model.request_test(M1));
    assert!(!model.request_test(M1), "one test of a row at a time");
    assert_eq!(model.lanes_to_start(), 1);
    let job = model.next_job().expect("a job");
    assert_eq!((job.host.as_str(), job.user.as_str()), (M1, "me"));
    assert_eq!(job.key, None);
    assert_eq!(job.tailnet, "person@example.com");
    assert!(model.next_job().is_none());
    model.finish_test(M1, job.round, SshTest::Connected { user: "me".into() });
    assert!(matches!(
        model.rows()[0].test,
        Some(TestState::Done(SshTest::Connected { .. }))
    ));
}

#[test]
fn test_selected_queues_every_checked_row_and_runs_at_most_four_at_once() {
    let mut model = ready(vec![]);
    assert!(!model.can_test());
    model.select_all();
    assert!(model.can_test());
    assert_eq!(model.request_selected(), 3);
    assert_eq!(model.lanes_to_start(), 3);
    model.lanes = 4;
    assert_eq!(model.lanes_to_start(), 0);
    model.lanes = 2;
    assert_eq!(model.lanes_to_start(), 2);
    assert_eq!(
        model.request_selected(),
        0,
        "rows already running are left alone"
    );
}

#[test]
fn a_changed_user_or_key_throws_the_old_result_away() {
    let mut model = ready(vec![]);
    model.toggle(M1);
    model.request_test(M1);
    let round = running_round(&model, M1);
    model.set_user(M1, "deploy");
    assert!(model.tests.is_empty());
    model.finish_test(M1, round, SshTest::Denied);
    assert!(
        model.tests.is_empty(),
        "a late answer for the old user is dropped"
    );
    model.request_test(M1);
    let job = model.next_job().unwrap();
    assert_eq!(job.user, "deploy");
    model.finish_test(M1, job.round, SshTest::Denied);
    model.set_key(M1, Some("~/.ssh/id_work"), Some(usable()));
    assert!(model.tests.is_empty());
    model.request_test(M1);
    assert_eq!(
        model.next_job().unwrap().key.as_deref(),
        Some("~/.ssh/id_work")
    );
}

#[test]
fn unchecking_a_row_forgets_its_key_test_and_place_in_line() {
    let mut model = ready(vec![]);
    model.toggle(M1);
    model.set_key(M1, Some("~/.ssh/id_work"), Some(usable()));
    model.request_test(M1);
    model.toggle(M1);
    assert!(model.keys.is_empty() && model.tests.is_empty() && model.waiting.is_empty());
    assert!(model.next_job().is_none());
}

#[test]
fn a_bad_key_stops_the_test_and_the_import() {
    let mut model = ready(vec![]);
    model.toggle(M1);
    model.set_key(
        M1,
        Some("~/.ssh/nope"),
        Some(KeyStatus {
            issue: Some(KeyIssue::Missing),
            public: false,
        }),
    );
    assert!(!model.can_import());
    assert_eq!(model.key_issue(), Some((M1.to_owned(), KeyIssue::Missing)));
    assert_eq!(model.rows()[0].key_issue, Some(KeyIssue::Missing));
    assert!(!model.request_test(M1));
    assert!(matches!(
        model.tests.get(M1),
        Some(TestState::Done(SshTest::Key(KeyIssue::Missing)))
    ));
    assert!(model.next_job().is_none());
    model.set_key(M1, None, None);
    assert!(model.can_import());
    model.set_key(M1, Some("relative"), None);
    assert!(!model.can_import());
    assert_eq!(model.rows()[0].key_issue, Some(KeyIssue::BadShape));
}

#[test]
fn a_public_key_only_warns() {
    let mut model = ready(vec![]);
    model.toggle(M1);
    model.set_key(
        M1,
        Some("~/.ssh/id_work.pub"),
        Some(KeyStatus {
            issue: None,
            public: true,
        }),
    );
    assert!(model.can_import());
    assert!(model.rows()[0].key_status.unwrap().public);
}

#[test]
fn an_import_saves_each_rows_key_and_leaves_the_rest_without_one() {
    let rig = Rig::new(vec![]);
    let mut model = ready(vec![]);
    model.toggle(M1);
    model.toggle(ORACLE);
    model.set_key(ORACLE, Some("~/.ssh/id_oracle"), Some(usable()));
    import_machines(&rig.parts(), &model.status, &model.picked(), None).unwrap();
    assert_eq!(rig.saved("m1").unwrap().ssh_key, None);
    assert_eq!(
        rig.saved("oracle-sg").unwrap().ssh_key.as_deref(),
        Some("~/.ssh/id_oracle")
    );
}

#[test]
fn the_results_and_keys_are_gone_after_an_import() {
    let mut model = ready(vec![]);
    model.toggle(M1);
    model.set_key(M1, Some("~/.ssh/id_work"), Some(usable()));
    model.request_test(M1);
    model.imported(1);
    assert!(model.keys.is_empty() && model.tests.is_empty() && model.waiting.is_empty());
}
