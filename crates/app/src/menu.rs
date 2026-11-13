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
pub struct MenuState {
    pub account: Option<String>,
    pub awake: AwakeMode,
    pub keep_display_on: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Accounts,
    OpenChat,
    SetAwake(AwakeMode),
    ToggleDisplay,
    Settings,
    About,
    Welcome,
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
    vec![
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
    ]
}

pub fn tray_symbol(state: &MenuState) -> &'static str {
    if state.awake == AwakeMode::Off {
        "cup.and.saucer"
    } else {
        "cup.and.saucer.fill"
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
        assert_eq!(tray_symbol(&MenuState::default()), "cup.and.saucer");
        let on = MenuState {
            awake: AwakeMode::On,
            ..MenuState::default()
        };
        assert_eq!(tray_symbol(&on), "cup.and.saucer.fill");
    }
}
