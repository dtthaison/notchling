use super::*;

fn row(id: &str, title: &str, allow: bool) -> BlockingRow {
    BlockingRow {
        id: id.to_owned(),
        title: title.to_owned(),
        who: "Claude Code".to_owned(),
        waited: "3m".to_owned(),
        allow_from_menu: allow,
        deny_from_menu: true,
    }
}

fn state(blocking: usize, waiting: usize, working: bool) -> MenuState {
    MenuState {
        blocking: (0..blocking)
            .map(|i| row(&format!("b{i}"), "Turn On Machines", true))
            .collect(),
        waiting_count: waiting,
        working,
        ..MenuState::default()
    }
}

fn names(entries: &[Entry]) -> Vec<String> {
    entries
        .iter()
        .map(|e| match e {
            Entry::Header(t) | Entry::Disabled(t) => t.clone(),
            Entry::Action { title, .. } | Entry::Submenu { title, .. } => title.clone(),
            Entry::Separator => "-".to_owned(),
        })
        .collect()
}

#[test]
fn the_worst_state_wins_for_all_three_dots() {
    assert_eq!(tray_look(&state(1, 1, true), false).dots, Dots::Blocked);
    assert_eq!(tray_look(&state(0, 1, true), false).dots, Dots::Waiting);
    assert_eq!(tray_look(&state(0, 0, true), false).dots, Dots::Working);
    assert_eq!(tray_look(&state(0, 0, false), false).dots, Dots::Idle);
}

#[test]
fn idle_keeps_the_outline_and_the_filled_glyph_as_today() {
    assert_eq!(
        tray_look(&MenuState::default(), false).glyph,
        TrayGlyph::Outline
    );
    let held = MenuState {
        holding: true,
        ..MenuState::default()
    };
    let look = tray_look(&held, false);
    assert_eq!((look.glyph, look.dots), (TrayGlyph::Filled, Dots::Idle));
}

#[test]
fn reduce_motion_never_pulses_and_only_blocked_pulses() {
    assert!(tray_look(&state(2, 0, false), false).pulse);
    assert!(!tray_look(&state(2, 0, false), true).pulse);
    assert!(!tray_look(&state(0, 2, true), false).pulse);
    assert!(!tray_look(&state(0, 0, true), false).pulse);
}

#[test]
fn the_look_counts_what_it_names() {
    assert_eq!(tray_look(&state(2, 5, true), false).count, 2);
    assert_eq!(tray_look(&state(0, 5, true), false).count, 5);
    assert_eq!(tray_look(&state(0, 0, true), false).count, 0);
}

#[test]
fn a_pulse_starts_when_blocked_begins_or_the_blocked_count_grows() {
    let none = tray_look(&state(0, 0, false), false);
    let one = tray_look(&state(1, 0, false), false);
    let two = tray_look(&state(2, 0, false), false);
    assert!(pulse_due(None, &one));
    assert!(pulse_due(Some(&none), &one));
    assert!(pulse_due(Some(&one), &two));
    assert!(!pulse_due(Some(&two), &two));
    assert!(!pulse_due(Some(&two), &one));
    assert!(!pulse_due(
        Some(&none),
        &tray_look(&state(1, 0, false), true)
    ));
}

#[test]
fn two_blocking_items_open_the_menu_on_a_header_and_two_submenus() {
    let menu = tray_menu(&state(2, 0, false));
    assert_eq!(names(&menu)[0], "Blocking 2 Tasks");
    assert!(matches!(menu[0], Entry::Header(_)));
    assert!(matches!(menu[1], Entry::Submenu { .. }));
    assert!(matches!(menu[2], Entry::Submenu { .. }));
    assert_eq!(menu[3], Entry::Separator);
    assert_eq!(names(&menu)[4], "Claude Code");
}

#[test]
fn one_blocking_item_says_task() {
    assert_eq!(names(&tray_menu(&state(1, 0, false)))[0], "Blocking 1 Task");
}

#[test]
fn nothing_blocking_leaves_the_menu_as_it_was() {
    assert_eq!(
        names(&tray_menu(&state(0, 3, true))),
        names(&tray_menu(&MenuState::default()))
    );
}

#[test]
fn a_row_that_may_not_be_allowed_from_the_menu_has_no_allow() {
    let current = MenuState {
        blocking: vec![
            row("model", "Switch Model", false),
            row("setting", "Change Notch Width", true),
        ],
        ..MenuState::default()
    };
    let menu = tray_menu(&current);
    let Entry::Submenu { title, items } = &menu[1] else {
        panic!("a submenu");
    };
    assert!(title.starts_with("Switch Model"));
    assert_eq!(names(items), ["Deny", "Change…", "-", "Open in Inbox"]);
    let Entry::Submenu { items, .. } = &menu[2] else {
        panic!("a submenu");
    };
    assert_eq!(
        names(items),
        ["Allow", "Deny", "Change…", "-", "Open in Inbox"]
    );
}

#[test]
fn a_submenu_acts_on_its_own_row() {
    let current = MenuState {
        blocking: vec![row("one", "Switch Model", true)],
        ..MenuState::default()
    };
    let menu = tray_menu(&current);
    let Entry::Submenu { items, .. } = &menu[1] else {
        panic!("a submenu");
    };
    let commands: Vec<Command> = items
        .iter()
        .filter_map(|e| match e {
            Entry::Action { command, .. } => Some(command.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        commands,
        [
            Command::Blocking(BlockAct::Allow, "one".to_owned()),
            Command::Blocking(BlockAct::Deny, "one".to_owned()),
            Command::Blocking(BlockAct::Change, "one".to_owned()),
            Command::Blocking(BlockAct::OpenInbox, "one".to_owned()),
        ]
    );
}

#[test]
fn a_row_titles_what_waits_who_asked_and_how_long() {
    let menu = tray_menu(&state(1, 0, false));
    let Entry::Submenu { title, .. } = &menu[1] else {
        panic!("a submenu");
    };
    assert_eq!(title, "Turn On Machines · Claude Code · 3m");
}

#[test]
fn a_row_with_no_agent_name_leaves_it_out() {
    let mut alone = row("a", "Turn On Machines", true);
    alone.who = String::new();
    let current = MenuState {
        blocking: vec![alone],
        ..MenuState::default()
    };
    let Entry::Submenu { title, .. } = &tray_menu(&current)[1] else {
        panic!("a submenu");
    };
    assert_eq!(title, "Turn On Machines · 3m");
}

#[test]
fn the_screen_reader_hears_the_state() {
    assert_eq!(
        a11y(&tray_look(&state(2, 0, true), false)),
        "Notchling, 2 blocked"
    );
    assert_eq!(
        a11y(&tray_look(&state(0, 1, true), false)),
        "Notchling, 1 waits on you"
    );
    assert_eq!(
        a11y(&tray_look(&state(0, 3, true), false)),
        "Notchling, 3 wait on you"
    );
    assert_eq!(
        a11y(&tray_look(&state(0, 0, true), false)),
        "Notchling, agents working"
    );
    assert_eq!(a11y(&tray_look(&state(0, 0, false), false)), "Notchling");
}

#[test]
fn a_row_with_no_deny_path_offers_no_deny() {
    let mut quiet = row("model", "Pick A Model", false);
    quiet.deny_from_menu = false;
    let menu = tray_menu(&MenuState {
        blocking: vec![quiet],
        ..MenuState::default()
    });
    let Entry::Submenu { items, .. } = &menu[1] else {
        panic!("no submenu");
    };
    assert_eq!(names(items), vec!["Change…", "-", "Open in Inbox"],);
}

#[test]
fn a_running_pulse_keeps_running_when_only_the_glyph_or_the_count_drops() {
    let two = tray_look(&state(2, 0, false), false);
    let one = tray_look(&state(1, 0, false), false);
    let held = tray_look(&state(1, 0, false), false);
    assert_eq!(pulse_action(true, Some(&two), &one), PulseAction::Keep);
    assert_eq!(pulse_action(true, Some(&one), &held), PulseAction::Keep);
    assert_eq!(pulse_action(false, Some(&two), &one), PulseAction::Stop);
}

#[test]
fn a_pulse_stops_when_blocked_ends_or_reduce_motion_turns_on() {
    let one = tray_look(&state(1, 0, false), false);
    let calm = tray_look(&state(1, 0, false), true);
    let waiting = tray_look(&state(0, 1, false), false);
    assert_eq!(pulse_action(true, Some(&one), &calm), PulseAction::Stop);
    assert_eq!(pulse_action(true, Some(&one), &waiting), PulseAction::Stop);
}

#[test]
fn a_pulse_starts_when_blocked_begins_or_grows() {
    let none = tray_look(&state(0, 0, false), false);
    let one = tray_look(&state(1, 0, false), false);
    let two = tray_look(&state(2, 0, false), false);
    assert_eq!(pulse_action(false, Some(&none), &one), PulseAction::Start);
    assert_eq!(pulse_action(true, Some(&one), &two), PulseAction::Start);
    assert_eq!(pulse_action(false, None, &one), PulseAction::Start);
}
