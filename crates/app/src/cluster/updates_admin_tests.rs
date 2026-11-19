use std::cell::RefCell;
use std::collections::BTreeMap;

use notchling_engine::accounts::Secret;
use notchling_engine::machines::registry::{AdminAccess, Machine, MachineOs};
use notchling_engine::vault::entry::{Kind, VaultEntry, VaultValue};

use super::*;

fn need(machine: &str, link: Option<&str>) -> Need {
    Need {
        machine: machine.into(),
        user: "worker".into(),
        host: format!("{machine}.tail1234.ts.net"),
        link: link.map(str::to_owned),
    }
}

fn items(names: &[&str]) -> VaultState {
    VaultState::Items(names.iter().map(|n| (*n).to_owned()).collect())
}

fn ask(source: Source) -> Ask {
    match source {
        Source::Ask(ask) => ask,
        other => panic!("{other:?}"),
    }
}

#[derive(Default)]
struct Fake {
    saved: RefCell<Vec<(VaultEntry, VaultValue)>>,
    links: RefCell<Vec<(String, Option<String>)>>,
    refuse: bool,
}

impl ItemSaver for Fake {
    fn save(&self, entry: &VaultEntry, value: &VaultValue) -> Result<(), String> {
        if self.refuse {
            return Err("the Keychain refused".into());
        }
        self.saved.borrow_mut().push((entry.clone(), value.clone()));
        Ok(())
    }
}

impl AdminLinks for Fake {
    fn link(&self, machine: &str, item: Option<&str>) -> Result<(), String> {
        self.links
            .borrow_mut()
            .push((machine.to_owned(), item.map(str::to_owned)));
        Ok(())
    }
}

fn passwords(list: &[(&str, &str)]) -> BTreeMap<String, Secret> {
    list.iter()
        .map(|(m, p)| ((*m).to_owned(), Secret::new(*p)))
        .collect()
}

#[test]
fn the_item_is_named_after_the_machine_and_never_takes_a_used_name() {
    assert_eq!(item_name("m1", &[]), "m1-admin");
    assert_eq!(item_name("m1", &["M1-Admin".into()]), "m1-admin-2");
    assert_eq!(
        item_name("m1", &["m1-admin".into(), "m1-admin-2".into()]),
        "m1-admin-3"
    );
    assert_eq!(item_name("1box", &[]), "machine-1box-admin");
    assert_eq!(item_name("my box", &[]), "my-box-admin");
    assert!(valid_name(&item_name(&"x".repeat(200), &[])));
}

fn valid_name(name: &str) -> bool {
    notchling_engine::vault::entry::valid_name(name)
}

#[test]
fn a_linked_item_that_is_in_the_vault_asks_for_nothing() {
    let n = need("m1", Some("m1-admin"));
    assert_eq!(
        source(&n, &items(&["npm", "M1-Admin"])),
        Source::Linked("m1-admin".into())
    );
    assert_eq!(
        source(&n, &VaultState::Reading),
        Source::Linked("m1-admin".into())
    );
}

#[test]
fn a_deleted_item_falls_back_to_asking_and_says_which() {
    let n = need("m1", Some("m1-admin"));
    let got = ask(source(&n, &items(&["npm"])));
    assert_eq!(got.gone.as_deref(), Some("m1-admin"));
    assert_eq!(got.saves_as.as_deref(), Some("m1-admin"));
}

#[test]
fn no_link_asks_and_saves_under_the_machines_name() {
    let got = ask(source(&need("m1", None), &items(&["npm"])));
    assert_eq!(got.gone, None);
    assert_eq!(got.saves_as.as_deref(), Some("m1-admin"));
}

#[test]
fn an_unavailable_vault_asks_and_saves_nothing() {
    let got = ask(source(
        &need("m1", Some("m1-admin")),
        &VaultState::Unavailable,
    ));
    assert_eq!(got.saves_as, None);
    assert_eq!(got.gone, None);
}

#[test]
fn the_update_waits_for_every_asked_password() {
    let a = need("m1", None);
    let b = need("m2", Some("m2-admin"));
    let vault = items(&["m2-admin"]);
    let sources: Vec<(Need, Source)> = [a, b]
        .into_iter()
        .map(|n| {
            let s = source(&n, &vault);
            (n, s)
        })
        .collect();
    let none = |_: &str| false;
    let blocked_now = blocked(&sources, &none, &vault).expect("m1 needs one");
    assert_eq!(blocked_now, Blocked::NeedsPassword(vec!["m1".into()]));
    assert_eq!(
        blocked_now.reason().as_deref(),
        Some("Type the admin password for m1 to update.")
    );
    let typed = |m: &str| m == "m1";
    assert_eq!(blocked(&sources, &typed, &vault), None);
}

#[test]
fn nothing_blocks_when_no_machine_needs_sudo_and_reading_blocks_only_with_machines() {
    assert_eq!(blocked(&[], &|_| false, &VaultState::Reading), None);
    let n = need("m1", Some("m1-admin"));
    let sources = vec![(n.clone(), source(&n, &VaultState::Reading))];
    assert_eq!(
        blocked(&sources, &|_| false, &VaultState::Reading),
        Some(Blocked::Reading)
    );
}

#[test]
fn only_sudo_machines_with_an_admin_step_are_listed() {
    let mut mac = Machine::new("m1", "m1.test", "worker");
    mac.os = Some(MachineOs::MacOs);
    mac.set_admin_secret(Some("m1-admin"));
    let mut vps = Machine::new("vps", "vps.test", "agent");
    vps.os = Some(MachineOs::Linux);
    vps.set_admin(AdminAccess::RootSsh);
    let got = needs(&[mac, vps], &["m1".into(), "vps".into(), "gone".into()]);
    assert_eq!(got, vec![need("m1", Some("m1-admin")).with_host("m1.test")]);
}

impl Need {
    fn with_host(mut self, host: &str) -> Need {
        self.host = host.into();
        self
    }
}

#[test]
fn a_typed_password_is_saved_as_a_login_item_and_linked() {
    let n = need("m1", None);
    let vault = items(&[]);
    let sources = vec![(n.clone(), source(&n, &vault))];
    let prepared = prepare(&sources, passwords(&[("m1", "hunter2")]));
    assert!(prepared.typed.is_empty());
    let fake = Fake::default();
    let typed = apply(prepared, &fake, &fake).expect("saved");
    assert!(typed.is_empty());
    let saved = fake.saved.borrow();
    let (entry, value) = &saved[0];
    assert_eq!(entry.name, "m1-admin");
    assert_eq!(entry.kind, Kind::Login);
    assert_eq!(entry.url.as_deref(), Some("m1.tail1234.ts.net"));
    assert_eq!(
        entry.note,
        "macOS admin password for m1, used by Notchling updates"
    );
    assert_eq!(value.username, "worker");
    assert_eq!(value.secret, "hunter2");
    assert_eq!(
        *fake.links.borrow(),
        vec![("m1".to_owned(), Some("m1-admin".to_owned()))]
    );
}

#[test]
fn without_a_vault_the_password_is_used_once_and_a_stale_link_is_dropped() {
    let n = need("m1", Some("m1-admin"));
    let sources = vec![(n.clone(), source(&n, &VaultState::Unavailable))];
    let prepared = prepare(&sources, passwords(&[("m1", "hunter2")]));
    let fake = Fake::default();
    let typed = apply(prepared, &fake, &fake).expect("applied");
    assert_eq!(typed["m1"].expose(), "hunter2");
    assert!(fake.saved.borrow().is_empty());
    assert_eq!(*fake.links.borrow(), vec![("m1".to_owned(), None)]);
}

#[test]
fn a_linked_machine_is_left_alone() {
    let n = need("m1", Some("m1-admin"));
    let vault = items(&["m1-admin"]);
    let sources = vec![(n.clone(), source(&n, &vault))];
    let prepared = prepare(&sources, passwords(&[("m1", "stale")]));
    assert_eq!(prepared, Prepared::default());
}

#[test]
fn a_failed_save_stops_before_anything_is_linked_and_never_quotes_the_password() {
    let n = need("m1", None);
    let sources = vec![(n.clone(), source(&n, &items(&[])))];
    let prepared = prepare(&sources, passwords(&[("m1", "hunter2")]));
    let fake = Fake {
        refuse: true,
        ..Fake::default()
    };
    let why = apply(prepared, &fake, &fake).expect_err("refused");
    assert!(why.contains("the Keychain refused"), "{why}");
    assert!(!why.contains("hunter2"));
    assert!(fake.links.borrow().is_empty());
}

#[test]
fn the_debug_of_a_prepared_update_hides_the_password() {
    let n = need("m1", None);
    let sources = vec![(n.clone(), source(&n, &items(&[])))];
    let prepared = prepare(&sources, passwords(&[("m1", "hunter2")]));
    assert!(!format!("{prepared:?}").contains("hunter2"));
}
