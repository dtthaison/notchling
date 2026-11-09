//! What the reply box's two controls do, pure: the action button at the right of the card's bottom
//! row (send, stop or nothing) and the items of the + menu at its left. `view::composer` draws
//! them.

use notchling_kit::Icon;

use crate::chat::data::Tool;

/// The action button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Nothing to send and nothing to stop.
    None,
    /// A return arrow: the box has text.
    Send,
    /// A solid square while the session works; dimmed while a Stop is on its way.
    Stop { dim: bool },
}

/// Which button shows. A working session shows Stop even with text typed (Return still queues the
/// text); an idle one shows Send once the box has more than blanks.
pub fn action(has_text: bool, working: bool, stopping: bool) -> Action {
    if working {
        Action::Stop { dim: stopping }
    } else if has_text {
        Action::Send
    } else {
        Action::None
    }
}

/// What a + menu item does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Add {
    /// A file picker; the paths go into the box at the caret.
    Files,
    /// The same for folders.
    Folder,
    /// A "/" at the start of the box.
    Slash,
}

/// One row of the + menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddItem {
    pub add: Add,
    pub title: &'static str,
    pub icon: Icon,
    /// The key shown at the right, when the item has one.
    pub shortcut: Option<&'static str>,
}

/// The + menu for a row's tool. Codex's turns are `codex exec`, which has no slash commands, so
/// its menu has no Slash Commands row. Connectors and Plugins (`/mcp`, `/plugin`) are not here:
/// they open Claude Code's own menus, which the Swift app runs in its terminal sheet and this app
/// has no terminal for yet.
pub fn add_menu(tool: Tool) -> Vec<AddItem> {
    let mut items = vec![
        AddItem {
            add: Add::Files,
            title: "Add files or photos",
            icon: Icon::Paperclip,
            shortcut: Some("⌘U"),
        },
        AddItem {
            add: Add::Folder,
            title: "Add folder",
            icon: Icon::Folder,
            shortcut: None,
        },
    ];
    if tool != Tool::Codex {
        items.push(AddItem {
            add: Add::Slash,
            title: "Slash commands",
            icon: Icon::SlashBox,
            shortcut: None,
        });
    }
    items
}

/// The box's text with a "/" at its start (as is when it starts with one already).
pub fn with_slash(text: &str) -> String {
    if text.starts_with('/') {
        text.to_owned()
    } else {
        format!("/{text}")
    }
}

/// Whether a key press is the Add Files shortcut: ⌘U, nothing else held.
pub fn is_add_files(key: &str, platform: bool, shift: bool, alt: bool, control: bool) -> bool {
    key == "u" && platform && !shift && !alt && !control
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_action_is_stop_while_working_else_send_with_text_else_nothing() {
        assert_eq!(action(false, false, false), Action::None);
        assert_eq!(action(true, false, false), Action::Send);
        assert_eq!(action(false, true, false), Action::Stop { dim: false });
        assert_eq!(action(true, true, false), Action::Stop { dim: false });
        assert_eq!(action(true, true, true), Action::Stop { dim: true });
        // A stop that lands after the turn ended no longer shows.
        assert_eq!(action(false, false, true), Action::None);
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
