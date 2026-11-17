use notchling_engine::accounts::Secret;

use super::planned;
use crate::cluster::add::Phase;
use crate::cluster::add_rig_tests::Rig;

#[test]
fn a_vault_entry_stands_in_for_the_typed_password() {
    let rig = Rig::new(vec![]);
    let mut flow = planned(&rig, "agent@vps-sing-1");
    assert!(flow.run(None).is_err());
    assert_eq!(flow.phase(), &Phase::Plan);
    flow.set_admin_entry(Some("m1-admin".into())).expect("set");
    assert!(flow.run(None).is_ok());
    assert!(matches!(flow.phase(), Phase::Running { .. }));
}

#[test]
fn the_chosen_entry_is_kept_on_the_machine() {
    let rig = Rig::new(vec![]);
    let mut flow = planned(&rig, "agent@vps-sing-1");
    flow.set_admin_entry(Some("m1-admin".into())).expect("set");
    let saved = rig.saved("vps-sing-1").expect("saved");
    assert_eq!(saved.admin_secret(), Some("m1-admin"));
    assert_eq!(
        flow.plan().unwrap().admin_entry.as_deref(),
        Some("m1-admin")
    );
    flow.set_admin_entry(None).expect("clear");
    assert_eq!(rig.saved("vps-sing-1").unwrap().admin_secret(), None);
    assert!(flow.run(None).is_err());
}

#[test]
fn a_typed_password_still_runs_with_an_entry_chosen() {
    let rig = Rig::new(vec![]);
    let mut flow = planned(&rig, "agent@vps-sing-1");
    flow.set_admin_entry(Some("m1-admin".into())).expect("set");
    assert!(flow.run(Some(Secret::new("test-password"))).is_ok());
}

#[test]
fn the_entry_cannot_change_once_the_run_started() {
    let rig = Rig::new(vec![]);
    let mut flow = planned(&rig, "agent@vps-sing-1");
    flow.run(Some(Secret::new("test-password"))).expect("runs");
    assert!(flow.set_admin_entry(Some("m1-admin".into())).is_err());
    assert_eq!(rig.saved("vps-sing-1").unwrap().admin_secret(), None);
}
