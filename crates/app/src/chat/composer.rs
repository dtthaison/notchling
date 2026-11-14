use notchling_kit::Icon;
use notchling_locale as t;

use crate::chat::data::Tool;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Send,
    Sending,
    Stop { dim: bool },
}

pub fn action(has_text: bool, working: bool, stopping: bool, sending: bool) -> Action {
    if working {
        Action::Stop { dim: stopping }
    } else if has_text {
        Action::Send
    } else if sending {
        Action::Sending
    } else {
        Action::None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Add {
    Files,
    Folder,
    Slash,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddItem {
    pub add: Add,
    pub title: &'static str,
    pub icon: Icon,
    pub shortcut: Option<&'static str>,
}

pub fn add_menu(tool: Tool) -> Vec<AddItem> {
    let mut items = vec![
        AddItem {
            add: Add::Files,
            title: t::chat::add_files(),
            icon: Icon::Paperclip,
            shortcut: Some("⌘U"),
        },
        AddItem {
            add: Add::Folder,
            title: t::chat::add_folder(),
            icon: Icon::Folder,
            shortcut: None,
        },
    ];
    if tool != Tool::Codex {
        items.push(AddItem {
            add: Add::Slash,
            title: t::chat::slash_commands(),
            icon: Icon::SlashBox,
            shortcut: None,
        });
    }
    items
}

pub fn with_slash(text: &str) -> String {
    if text.starts_with('/') {
        text.to_owned()
    } else {
        format!("/{text}")
    }
}

pub fn is_add_files(key: &str, platform: bool, shift: bool, alt: bool, control: bool) -> bool {
    key == "u" && platform && !shift && !alt && !control
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_action_is_stop_while_working_else_send_with_text_else_nothing() {
        assert_eq!(action(false, false, false, false), Action::None);
        assert_eq!(action(true, false, false, false), Action::Send);
        assert_eq!(
            action(false, true, false, false),
            Action::Stop { dim: false }
        );
        assert_eq!(
            action(true, true, false, false),
            Action::Stop { dim: false }
        );
        assert_eq!(action(true, true, true, false), Action::Stop { dim: true });
        assert_eq!(action(false, false, true, false), Action::None);
        assert_eq!(action(false, false, false, true), Action::Sending);
        assert_eq!(action(true, false, false, true), Action::Send);
        assert_eq!(
            action(false, true, false, true),
            Action::Stop { dim: false }
        );
    }

    #[test]
    fn the_menu_has_files_folder_and_slash_but_codex_has_no_slash() {
        let claude: Vec<_> = add_menu(Tool::Claude).iter().map(|i| i.add).collect();
        assert_eq!(claude, [Add::Files, Add::Folder, Add::Slash]);
        let agy: Vec<_> = add_menu(Tool::Antigravity).iter().map(|i| i.add).collect();
        assert_eq!(agy, claude);
        let codex: Vec<_> = add_menu(Tool::Codex).iter().map(|i| i.add).collect();
        assert_eq!(codex, [Add::Files, Add::Folder]);
        assert_eq!(add_menu(Tool::Claude)[0].shortcut, Some("⌘U"));
    }

    #[test]
    fn a_slash_goes_in_front_once() {
        assert_eq!(with_slash(""), "/");
        assert_eq!(with_slash("review"), "/review");
        assert_eq!(with_slash("/model"), "/model");
    }

    #[test]
    fn only_a_bare_command_u_adds_files() {
        assert!(is_add_files("u", true, false, false, false));
        assert!(!is_add_files("u", false, false, false, false));
        assert!(!is_add_files("u", true, true, false, false));
        assert!(!is_add_files("i", true, false, false, false));
    }
}
