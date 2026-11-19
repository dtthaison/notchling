use notchling_engine::machines::addresses::{AddressKind, MachineAddress};
use notchling_engine::machines::registry::Machine;
use notchling_engine::machines::ssh_key::KeyIssue;

use super::add_rig_tests::Rig;
use super::edit_key::{draft_machine, key_of, save_edit};
use crate::testutil::Scratch;

#[cfg(unix)]
fn key_file(scratch: &Scratch, name: &str, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let path = scratch.write(name, "a fixture, not a key");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn an_empty_field_means_no_key() {
    let scratch = Scratch::new("editkey");
    let (key, status) = key_of("   ", scratch.path()).unwrap();
    assert_eq!(key, None);
    assert!(status.is_usable());
}

#[cfg(unix)]
#[test]
fn a_typed_or_chosen_path_is_checked_and_stored_with_its_tilde() {
    let scratch = Scratch::new("editkey");
    key_file(&scratch, ".ssh/id_work", 0o600);
    key_file(&scratch, ".ssh/id_open", 0o644);
    key_file(&scratch, ".ssh/id_work.pub", 0o644);
    let home = scratch.path();
    let absolute = home.join(".ssh/id_work").to_string_lossy().into_owned();
    assert_eq!(
        key_of(&absolute, home).unwrap().0.as_deref(),
        Some("~/.ssh/id_work")
    );
    assert_eq!(
        key_of("~/.ssh/id_work", home).unwrap().0.as_deref(),
        Some("~/.ssh/id_work")
    );
    assert_eq!(
        key_of("~/.ssh/id_open", home).unwrap_err(),
        KeyIssue::TooOpen
    );
    assert_eq!(key_of("~/.ssh/gone", home).unwrap_err(), KeyIssue::Missing);
    assert_eq!(key_of("id_work", home).unwrap_err(), KeyIssue::BadShape);
    let (key, status) = key_of("~/.ssh/id_work.pub", home).unwrap();
    assert_eq!(key.as_deref(), Some("~/.ssh/id_work.pub"));
    assert!(status.public);
}

#[test]
fn a_test_draft_holds_the_unsaved_addresses_and_key_without_touching_the_machine() {
    let mut machine = Machine::new("m1", "m1.tail", "agent");
    machine.set_ssh_key(Some("~/.ssh/old"));
    let draft = draft_machine(
        &machine,
        vec![MachineAddress::new(AddressKind::Lan, "10.0.0.8")],
        Some("~/.ssh/new"),
    );
    assert_eq!(draft.host, "10.0.0.8");
    assert_eq!(draft.ssh_key(), Some("~/.ssh/new"));
    assert_eq!(machine.ssh_key(), Some("~/.ssh/old"));
    assert_eq!(machine.host, "m1.tail");
}

#[test]
fn saving_the_edit_stores_the_key_and_clearing_it_removes_it() {
    let rig = Rig::new(vec![Machine::new("m1", "m1.tail", "agent")]);
    let list = vec![MachineAddress::new(AddressKind::Lan, "10.0.0.8")];
    save_edit(&rig.parts(), "m1", list.clone(), Some("~/.ssh/id_work")).unwrap();
    let saved = rig.saved("m1").unwrap();
    assert_eq!(saved.ssh_key.as_deref(), Some("~/.ssh/id_work"));
    assert_eq!(saved.addresses, list);
    save_edit(&rig.parts(), "m1", list.clone(), None).unwrap();
    assert_eq!(rig.saved("m1").unwrap().ssh_key, None);
    assert!(save_edit(&rig.parts(), "nope", list, None).is_err());
}

#[test]
fn only_the_key_changing_is_a_change() {
    let rig = Rig::new(vec![Machine::new("m1", "m1.tail", "agent")]);
    let list = vec![MachineAddress::new(AddressKind::Lan, "10.0.0.8")];
    save_edit(&rig.parts(), "m1", list.clone(), None).unwrap();
    let before = rig.saved("m1").unwrap();
    save_edit(&rig.parts(), "m1", list.clone(), None).unwrap();
    assert_eq!(rig.saved("m1").unwrap(), before);
    save_edit(&rig.parts(), "m1", list, Some("~/.ssh/k")).unwrap();
    assert_eq!(
        rig.saved("m1").unwrap().ssh_key.as_deref(),
        Some("~/.ssh/k")
    );
}

#[test]
fn a_typed_key_is_only_shape_checked_on_the_ui_thread() {
    let home = std::path::Path::new("/nonexistent-home");
    assert_eq!(super::edit_key::key_text("  ", home), Ok(None));
    assert_eq!(
        super::edit_key::key_text("/nonexistent-home/.ssh/id_x", home),
        Ok(Some("~/.ssh/id_x".to_owned()))
    );
    assert_eq!(
        super::edit_key::key_text("id_x", home),
        Err(KeyIssue::BadShape)
    );
    assert_eq!(
        super::edit_key::key_text("~/.ssh/a%h", home),
        Err(KeyIssue::BadShape)
    );
}

#[test]
fn the_signature_changes_with_an_address_row_or_the_key() {
    use super::addresses_form::{RowDraft, RowMeta};
    use super::edit_key::signature;
    let row = |text: &str| RowDraft {
        text: text.into(),
        meta: RowMeta::default(),
    };
    let base = signature(&[row("10.0.0.1")], "~/.ssh/a");
    assert_eq!(base, signature(&[row("10.0.0.1")], " ~/.ssh/a "));
    assert_ne!(base, signature(&[row("10.0.0.2")], "~/.ssh/a"));
    assert_ne!(
        base,
        signature(&[row("10.0.0.1"), row("10.0.0.2")], "~/.ssh/a")
    );
    assert_ne!(base, signature(&[row("10.0.0.1")], "~/.ssh/b"));
}
