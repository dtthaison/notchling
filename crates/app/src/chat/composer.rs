use notchling_kit::Icon;
use notchling_locale as t;

use crate::chat::data::{Session, Tool};

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

pub fn add_allowed(add: Add, editing: bool) -> bool {
    !editing || add == Add::Slash
}

pub fn add_menu_for(tool: Tool, editing: bool) -> Vec<AddItem> {
    add_menu(tool)
        .into_iter()
        .filter(|item| add_allowed(item.add, editing))
        .collect()
}

pub fn takes_drops(in_subagent: bool, composer_on: bool, editing: bool) -> bool {
    !in_subagent && composer_on && !editing
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

pub fn can_resume_session(session: &Session) -> bool {
    session.tool == Tool::Claude
        && session.agent.can_resume()
        && !crate::chat::remote::is_remote(&session.agent)
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
    fn editing_an_outbox_row_offers_no_files_or_folders() {
        let claude: Vec<_> = add_menu_for(Tool::Claude, true)
            .iter()
            .map(|i| i.add)
            .collect();
        assert_eq!(claude, [Add::Slash]);
        assert!(add_menu_for(Tool::Codex, true).is_empty());
        assert_eq!(add_menu_for(Tool::Codex, false), add_menu(Tool::Codex));
        assert_eq!(add_menu_for(Tool::Claude, false), add_menu(Tool::Claude));
        assert!(!add_allowed(Add::Files, true));
        assert!(!add_allowed(Add::Folder, true));
        assert!(add_allowed(Add::Slash, true));
        assert!(add_allowed(Add::Files, false));
    }

    #[test]
    fn nothing_is_dropped_on_the_column_while_editing_or_in_a_subagent() {
        assert!(takes_drops(false, true, false));
        assert!(!takes_drops(false, true, true));
        assert!(!takes_drops(true, true, false));
        assert!(!takes_drops(false, false, false));
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

    #[test]
    fn only_an_ended_local_claude_session_offers_resume() {
        use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget, NodeRef};

        let base = Session {
            agent: Agent {
                id: "s1".into(),
                name: "fix".into(),
                project: "app".into(),
                cwd: "/work/app".into(),
                group: AgentGroup::Completed,
                label: "Exited".into(),
                detail: None,
                since_ms: 0,
                target: AgentTarget::Resume {
                    session_id: "s1".into(),
                    cwd: "/work/app".into(),
                },
                suggested_reply: None,
                transcript_path: None,
                host: String::new(),
                headless: false,
                config_dir: None,
                command: None,
                resume_session_id: None,
                at_prompt: false,
                node: None,
                worktree_path: None,
            },
            tool: Tool::Claude,
            transcript: None,
        };
        assert!(can_resume_session(&base));

        let mut codex = base.clone();
        codex.tool = Tool::Codex;
        assert!(!can_resume_session(&codex));

        let mut agy = base.clone();
        agy.tool = Tool::Antigravity;
        assert!(!can_resume_session(&agy));

        let mut running = base.clone();
        running.agent.target = AgentTarget::Job("j1".into());
        assert!(!can_resume_session(&running));

        let mut headless = base.clone();
        headless.agent.headless = true;
        assert!(!can_resume_session(&headless));

        let mut remote = base.clone();
        remote.agent.node = Some(NodeRef {
            id: "0123456789abcdef".into(),
            name: "w1".into(),
            online: true,
        });
        assert!(!can_resume_session(&remote));

        let mut qualified = base;
        qualified.agent.id = "0123456789abcdef/s1".into();
        assert!(!can_resume_session(&qualified));
    }
}
