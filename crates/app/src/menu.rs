use notchling_locale as t;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AwakeMode {
    #[default]
    Off,
    On,
    WhileWorking,
}

impl AwakeMode {
    pub const ALL: [AwakeMode; 3] = [AwakeMode::Off, AwakeMode::On, AwakeMode::WhileWorking];

    pub fn title(self) -> &'static str {
        match self {
            AwakeMode::Off => "Off",
            AwakeMode::On => "Always On",
            AwakeMode::WhileWorking => "While Agents Are Working",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlockingRow {
    pub id: String,
    pub title: String,
    pub who: String,
    pub waited: String,
    pub allow_from_menu: bool,
    pub deny_from_menu: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuState {
    pub account: Option<String>,
    pub awake: AwakeMode,
    pub keep_display_on: bool,
    pub holding: bool,
    pub update: Option<String>,
    pub blocking: Vec<BlockingRow>,
    pub waiting_count: usize,
    pub working: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockAct {
    Allow,
    Deny,
    Change,
    OpenInbox,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Blocking(BlockAct, String),
    Accounts,
    OpenChat,
    SetAwake(AwakeMode),
    ToggleDisplay,
    Settings,
    About,
    Welcome,
    OpenUpdate,
    Quit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Header(String),
    Disabled(String),
    Action {
        title: String,
        command: Command,
        key: Option<char>,
        checked: bool,
    },
    Submenu {
        title: String,
        items: Vec<Entry>,
    },
    Separator,
}

fn action(title: &str, command: Command, key: Option<char>, checked: bool) -> Entry {
    Entry::Action {
        title: title.to_owned(),
        command,
        key,
        checked,
    }
}

fn blocking_entries(rows: &[BlockingRow]) -> Vec<Entry> {
    if rows.is_empty() {
        return Vec::new();
    }
    let mut entries = vec![Entry::Header(t::tray::blocking_header(rows.len()))];
    for row in rows {
        let title = if row.who.is_empty() {
            t::tray::blocking_row_alone(&row.title, &row.waited)
        } else {
            t::tray::blocking_row(&row.title, &row.who, &row.waited)
        };
        let act = |title: &str, act: BlockAct| {
            action(title, Command::Blocking(act, row.id.clone()), None, false)
        };
        let mut items = Vec::new();
        if row.allow_from_menu {
            items.push(act(t::consent::allow(), BlockAct::Allow));
        }
        if row.deny_from_menu {
            items.push(act(t::consent::deny(), BlockAct::Deny));
        }
        items.push(act(t::consent::tray_change(), BlockAct::Change));
        items.push(Entry::Separator);
        items.push(act(t::tray::open_in_inbox(), BlockAct::OpenInbox));
        entries.push(Entry::Submenu { title, items });
    }
    entries.push(Entry::Separator);
    entries
}

pub fn tray_menu(state: &MenuState) -> Vec<Entry> {
    let account = match &state.account {
        Some(email) => action(email, Command::Accounts, None, false),
        None => Entry::Disabled("Not signed in to Claude Code".to_owned()),
    };
    let mut awake: Vec<Entry> = AwakeMode::ALL
        .iter()
        .map(|mode| {
            action(
                mode.title(),
                Command::SetAwake(*mode),
                None,
                state.awake == *mode,
            )
        })
        .collect();
    awake.push(Entry::Separator);
    awake.push(action(
        "Keep Display On",
        Command::ToggleDisplay,
        None,
        state.keep_display_on,
    ));
    let mut entries = blocking_entries(&state.blocking);
    entries.extend([
        Entry::Header("Claude Code".to_owned()),
        account,
        Entry::Separator,
        action("Open Chat", Command::OpenChat, Some('o'), false),
        Entry::Submenu {
            title: "Keep Mac Awake".to_owned(),
            items: awake,
        },
        action("Settings…", Command::Settings, Some(','), false),
        Entry::Submenu {
            title: "About".to_owned(),
            items: vec![
                action("About Notchling", Command::About, None, false),
                action("Welcome…", Command::Welcome, None, false),
            ],
        },
        Entry::Separator,
        action("Quit Notchling", Command::Quit, Some('q'), false),
    ]);
    if let Some(version) = &state.update {
        entries.insert(
            entries.len() - 2,
            action(
                &t::update::menu_available(version),
                Command::OpenUpdate,
                None,
                false,
            ),
        );
    }
    entries
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayGlyph {
    Outline,
    Filled,
}

pub fn tray_glyph(state: &MenuState) -> TrayGlyph {
    if state.holding {
        TrayGlyph::Filled
    } else {
        TrayGlyph::Outline
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dots {
    Idle,
    Working,
    Waiting,
    Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrayLook {
    pub glyph: TrayGlyph,
    pub dots: Dots,
    pub pulse: bool,
    pub count: usize,
}

pub fn tray_look(state: &MenuState, reduce_motion: bool) -> TrayLook {
    let (dots, count) = if !state.blocking.is_empty() {
        (Dots::Blocked, state.blocking.len())
    } else if state.waiting_count > 0 {
        (Dots::Waiting, state.waiting_count)
    } else if state.working {
        (Dots::Working, 0)
    } else {
        (Dots::Idle, 0)
    };
    TrayLook {
        glyph: tray_glyph(state),
        dots,
        pulse: dots == Dots::Blocked && !reduce_motion,
        count,
    }
}

pub fn pulse_due(before: Option<&TrayLook>, now: &TrayLook) -> bool {
    now.pulse && before.is_none_or(|b| b.dots != Dots::Blocked || now.count > b.count)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PulseAction {
    Start,
    Keep,
    Stop,
}

pub fn pulse_action(running: bool, before: Option<&TrayLook>, now: &TrayLook) -> PulseAction {
    if !now.pulse || now.dots != Dots::Blocked {
        PulseAction::Stop
    } else if pulse_due(before, now) {
        PulseAction::Start
    } else if running {
        PulseAction::Keep
    } else {
        PulseAction::Stop
    }
}

pub fn a11y(look: &TrayLook) -> String {
    match look.dots {
        Dots::Blocked => t::tray::a11y_blocked(&look.count.to_string()),
        Dots::Waiting => t::tray::a11y_waiting(look.count),
        Dots::Working => t::tray::a11y_working().to_owned(),
        Dots::Idle => t::tray::a11y_idle().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(entries: &[Entry]) -> Vec<String> {
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
    fn order_matches_the_swift_menu() {
        let menu = tray_menu(&MenuState::default());
        assert_eq!(
            titles(&menu),
            [
                "Claude Code",
                "Not signed in to Claude Code",
                "-",
                "Open Chat",
                "Keep Mac Awake",
                "Settings…",
                "About",
                "-",
                "Quit Notchling"
            ]
        );
    }

    #[test]
    fn a_newer_release_adds_one_row_before_quit_and_none_otherwise() {
        let state = MenuState {
            update: Some("2.1.0".to_owned()),
            ..MenuState::default()
        };
        let menu = tray_menu(&state);
        assert_eq!(menu.len(), tray_menu(&MenuState::default()).len() + 1);
        assert_eq!(
            menu[menu.len() - 3],
            action("Update Available: 2.1.0…", Command::OpenUpdate, None, false)
        );
        assert_eq!(
            titles(&menu).last().map(String::as_str),
            Some("Quit Notchling")
        );
    }

    #[test]
    fn the_account_in_use_is_a_row() {
        let state = MenuState {
            account: Some("me@example.com".to_owned()),
            ..MenuState::default()
        };
        assert_eq!(
            tray_menu(&state)[1],
            action("me@example.com", Command::Accounts, None, false)
        );
    }

    #[test]
    fn awake_submenu_checks_the_mode_and_the_display() {
        let state = MenuState {
            awake: AwakeMode::WhileWorking,
            keep_display_on: true,
            ..MenuState::default()
        };
        let Entry::Submenu { items, .. } = &tray_menu(&state)[4] else {
            panic!("Keep Mac Awake is a submenu");
        };
        assert_eq!(
            titles(items),
            [
                "Off",
                "Always On",
                "While Agents Are Working",
                "-",
                "Keep Display On"
            ]
        );
        let checked: Vec<bool> = items
            .iter()
            .filter_map(|e| match e {
                Entry::Action { checked, .. } => Some(*checked),
                _ => None,
            })
            .collect();
        assert_eq!(checked, [false, false, true, true]);
    }

    #[test]
    fn keys_and_icon() {
        let menu = tray_menu(&MenuState::default());
        let keys: Vec<Option<char>> = menu
            .iter()
            .filter_map(|e| match e {
                Entry::Action { key, .. } => Some(*key),
                _ => None,
            })
            .collect();
        assert_eq!(keys, [Some('o'), Some(','), Some('q')]);
        assert_eq!(tray_glyph(&MenuState::default()), TrayGlyph::Outline);
        let chosen = MenuState {
            awake: AwakeMode::On,
            ..MenuState::default()
        };
        assert_eq!(tray_glyph(&chosen), TrayGlyph::Outline);
        let held = MenuState {
            holding: true,
            ..chosen
        };
        assert_eq!(tray_glyph(&held), TrayGlyph::Filled);
    }
}

#[cfg(test)]
#[path = "menu_tray_tests.rs"]
mod tray_tests;
