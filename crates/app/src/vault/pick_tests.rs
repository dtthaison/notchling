use notchling_engine::vault::entry::{Kind, VaultEntry};

use super::*;
use crate::switches::Switches;
use crate::vault::store::Vault;

fn entries() -> Vec<VaultEntry> {
    let mut gh = VaultEntry::new("m1-admin", Kind::Login);
    gh.url = Some("https://example.test/login".into());
    vec![
        VaultEntry::new("Zed", Kind::Token),
        gh,
        VaultEntry::new("alpha", Kind::Token),
    ]
}

#[test]
fn opening_starts_a_read_and_the_list_is_sorted_by_name_with_the_chosen_one_checked() {
    let mut pick = VaultPick::default();
    assert!(!pick.is_open());
    assert!(pick.toggle());
    assert_eq!(pick.note(), Some(PickNote::Reading));
    assert!(pick.rows(None).is_empty());
    pick.loaded(Ok(entries()));
    assert_eq!(pick.note(), None);
    let rows = pick.rows(Some("M1-ADMIN"));
    let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["alpha", "m1-admin", "Zed"]);
    assert_eq!(
        rows.iter().map(|r| r.checked).collect::<Vec<_>>(),
        [false, true, false]
    );
    assert_eq!(rows[1].id, "m1-admin");
    assert_eq!(rows[2].id, "Zed", "a choice carries the entry's own name");
    assert_eq!(rows[1].detail, "Login · example.test");
    assert_eq!(rows[0].detail, "Token");
    assert!(!pick.toggle());
}

#[test]
fn a_chosen_entry_the_vault_no_longer_has_stays_listed_so_it_can_be_cleared() {
    let mut pick = VaultPick::default();
    pick.toggle();
    pick.loaded(Ok(entries()));
    let rows = pick.rows(Some("Gone-Entry"));
    let last = rows.last().unwrap();
    assert!((last.gone, last.checked) == (true, true));
    assert_eq!(last.detail, "Not in the Vault");
    assert_eq!(
        (last.id.as_str(), last.name.as_str()),
        ("Gone-Entry", "Gone-Entry")
    );
    assert_eq!(rows.len(), 4);
}

#[test]
fn an_empty_or_failed_read_says_so_and_lists_nothing() {
    let mut pick = VaultPick::default();
    pick.toggle();
    pick.loaded(Ok(Vec::new()));
    assert_eq!(pick.note(), Some(PickNote::Empty));
    assert_eq!(pick.note().unwrap().text(), "The Vault has no entries yet.");
    pick.loaded(Err("The vault is off.".into()));
    assert_eq!(
        pick.note().unwrap().text(),
        "Couldn't read the Vault: The vault is off."
    );
    assert!(pick.rows(Some("x")).is_empty());
}

#[test]
fn a_late_read_after_closing_before_any_open_is_dropped() {
    let mut pick = VaultPick::default();
    pick.loaded(Ok(entries()));
    assert_eq!(pick.note(), Some(PickNote::Reading));
    assert!(pick.rows(None).is_empty());
}

#[test]
fn the_local_vault_is_read_only_while_its_switch_is_on() {
    let vault = Vault::fixed(entries());
    assert_eq!(local_entries(&vault, &Switches::all_on()).unwrap().len(), 3);
    assert_eq!(
        local_entries(&vault, &Switches::all_off()).unwrap_err(),
        crate::switches::Switch::Vault.off_message()
    );
    assert!(local_entries(&vault, &Switches::detached()).is_err());
}

#[test]
fn the_using_line_names_the_entry() {
    assert_eq!(
        using_line("m1-admin"),
        "Using m1-admin from the Vault. A password typed here is used instead."
    );
}
