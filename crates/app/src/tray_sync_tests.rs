use super::*;
use crate::menu::{tray_look, BlockingRow, MenuState};

#[derive(Default)]
struct Fake {
    looks: usize,
    menus: usize,
}

impl TrayPort for Fake {
    fn set_look(&mut self, _: &TrayLook) {
        self.looks += 1;
    }
    fn set_menu(&mut self, _: &MenuState) {
        self.menus += 1;
    }
}

fn blocked(count: usize) -> MenuState {
    MenuState {
        blocking: (0..count)
            .map(|i| BlockingRow {
                id: i.to_string(),
                title: "t".to_owned(),
                who: "w".to_owned(),
                waited: "1m".to_owned(),
                allow_from_menu: false,
                deny_from_menu: true,
            })
            .collect(),
        ..MenuState::default()
    }
}

#[test]
fn ten_look_changes_with_an_unchanged_menu_set_the_menu_once() {
    let (mut sync, mut fake) = (TraySync::default(), Fake::default());
    let menu = MenuState::default();
    for n in 0..10 {
        let look = tray_look(
            &MenuState {
                working: n % 2 == 0,
                waiting_count: n % 3,
                ..MenuState::default()
            },
            false,
        );
        sync.refresh(&mut fake, look, &menu);
    }
    assert_eq!(fake.menus, 1);
    assert!(fake.looks >= 5);
}

#[test]
fn an_unchanged_look_is_set_once() {
    let (mut sync, mut fake) = (TraySync::default(), Fake::default());
    let state = blocked(1);
    for _ in 0..5 {
        sync.refresh(&mut fake, tray_look(&state, false), &state);
    }
    assert_eq!((fake.looks, fake.menus), (1, 1));
}

#[test]
fn a_changed_menu_with_the_same_look_sets_only_the_menu() {
    let (mut sync, mut fake) = (TraySync::default(), Fake::default());
    let (one, other) = (blocked(1), {
        let mut s = blocked(1);
        s.blocking[0].waited = "2m".to_owned();
        s
    });
    sync.refresh(&mut fake, tray_look(&one, false), &one);
    sync.refresh(&mut fake, tray_look(&other, false), &other);
    assert_eq!((fake.looks, fake.menus), (1, 2));
}
