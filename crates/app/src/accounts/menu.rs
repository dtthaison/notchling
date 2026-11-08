//! Chat's header account menu, pure (the Swift `ChatAccountMenu`): which account this session
//! runs on, the targets it can move to (the main login, each lane, each free saved account), Move
//! All Sessions To, Add Account… and Codex Accounts…. Picking a target is Milestone 3's
//! `session.move` (acting), after a lane is made for a free account (`lanes.create`).

use super::store::Snapshot;

/// Where a session can run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// The main login (`~/.claude`).
    Main,
    /// A lane, by its config folder.
    Lane(String),
    /// A saved account not live anywhere yet: a lane is made for it first.
    Saved(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// The email, or "Main Login" when nobody is signed in there.
    pub title: String,
    pub place: Place,
    /// The account's id, when known.
    pub account: Option<String>,
}

/// The targets in the Swift order: the main login, the lanes, then the free saved accounts.
pub fn targets(s: &Snapshot) -> Vec<Target> {
    let active = s
        .claude
        .accounts
        .iter()
        .find(|a| Some(&a.id) == s.claude.active.as_ref());
    let mut out = vec![Target {
        title: active
            .map(|a| a.email.clone())
            .unwrap_or_else(|| "Main Login".to_owned()),
        place: Place::Main,
        account: active.map(|a| a.id.clone()),
    }];
    for lane in &s.lanes {
        out.push(Target {
            title: lane.email.clone().unwrap_or_else(|| lane.name.clone()),
            place: Place::Lane(lane.path.clone()),
            account: lane.account_id.clone(),
        });
    }
    for a in &s.claude.accounts {
        let live = Some(&a.id) == s.claude.active.as_ref() || a.lane.is_some();
        if !live && !a.needs_login {
            out.push(Target {
                title: a.email.clone(),
                place: Place::Saved(a.id.clone()),
                account: Some(a.id.clone()),
            });
        }
    }
    out
}

/// One line of the menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Header(&'static str),
    /// A target; `current` is checked and does nothing.
    Target {
        index: usize,
        current: bool,
    },
    Note(&'static str),
    Separator,
    /// "Move All Sessions To" with every target.
    MoveAll,
    AddAccount,
    CodexAccounts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountMenu {
    /// The email shown on the button.
    pub label: String,
    /// The lane's name when the session runs in one.
    pub lane: Option<String>,
    pub tooltip: String,
    pub targets: Vec<Target>,
    pub entries: Vec<Entry>,
}

/// The menu for a session. `config_dir` is the session's lane folder (`None` on the main
/// login); `movable` is a background job with a transcript, which can be moved (a terminal
/// session can't); `codex` says the Codex CLI is installed.
pub fn account_menu(
    s: &Snapshot,
    config_dir: Option<&str>,
    movable: bool,
    codex: bool,
) -> AccountMenu {
    let targets = targets(s);
    let lane = config_dir.and_then(|dir| s.lanes.iter().find(|l| l.path == dir));
    let current = match lane {
        Some(l) => targets
            .iter()
            .position(|t| t.place == Place::Lane(l.path.clone()))
            .unwrap_or(0),
        None => 0,
    };
    let label = targets[current].title.clone();
    let lane_name = lane.map(|l| l.name.clone());
    let tooltip = match &lane_name {
        Some(name) => {
            format!("This session runs in the {name} lane, on {label}. Click to switch it.")
        }
        None => "This session runs on the main login. Click to switch it.".to_owned(),
    };
    let mut entries = vec![Entry::Header("This Session's Account")];
    if movable {
        for i in 0..targets.len() {
            entries.push(Entry::Target {
                index: i,
                current: i == current,
            });
        }
        entries.push(Entry::Note(
            "Picking one moves only this session; the conversation continues there.",
        ));
    } else {
        entries.push(Entry::Target {
            index: current,
            current: true,
        });
    }
    if targets.len() > 1 {
        entries.push(Entry::Separator);
        entries.push(Entry::MoveAll);
    }
    entries.push(Entry::Separator);
    entries.push(Entry::AddAccount);
    if codex {
        entries.push(Entry::CodexAccounts);
    }
    AccountMenu {
        label,
        lane: lane_name,
        tooltip,
        targets,
        entries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::wire::{ClaudeAccount, ClaudeList, Lane};

    fn snapshot() -> Snapshot {
        Snapshot {
            attached: true,
            claude: ClaudeList {
                on: true,
                active: Some("a1".into()),
                accounts: vec![
                    ClaudeAccount {
                        id: "a1".into(),
                        email: "me@example.com".into(),
                        ..ClaudeAccount::default()
                    },
                    ClaudeAccount {
                        id: "a2".into(),
                        email: "ops@example.com".into(),
                        lane: Some("night".into()),
                        ..ClaudeAccount::default()
                    },
                    ClaudeAccount {
                        id: "a3".into(),
                        email: "team@example.com".into(),
                        ..ClaudeAccount::default()
                    },
                    ClaudeAccount {
                        id: "a4".into(),
                        email: "old@example.com".into(),
                        needs_login: true,
                        ..ClaudeAccount::default()
                    },
                ],
            },
            lanes: vec![Lane {
                name: "night".into(),
                path: "/h/.claude-lanes/night".into(),
                account_id: Some("a2".into()),
                email: Some("ops@example.com".into()),
            }],
            ..Snapshot::default()
        }
    }

    #[test]
    fn targets_are_the_main_login_the_lanes_then_the_free_accounts() {
        let t = targets(&snapshot());
        let titles: Vec<&str> = t.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            ["me@example.com", "ops@example.com", "team@example.com"],
            "a signed-out account is no target"
        );
        assert_eq!(t[0].place, Place::Main);
        assert_eq!(t[1].place, Place::Lane("/h/.claude-lanes/night".into()));
        assert_eq!(t[2].place, Place::Saved("a3".into()));
        let nobody = targets(&Snapshot::default());
        assert_eq!(nobody[0].title, "Main Login");
    }

    #[test]
    fn a_job_lists_every_target_and_a_terminal_only_its_own() {
        let s = snapshot();
        let job = account_menu(&s, Some("/h/.claude-lanes/night"), true, true);
        assert_eq!(job.label, "ops@example.com");
        assert_eq!(job.lane.as_deref(), Some("night"));
        assert_eq!(
            job.tooltip,
            "This session runs in the night lane, on ops@example.com. Click to switch it."
        );
        assert_eq!(
            job.entries,
            vec![
                Entry::Header("This Session's Account"),
                Entry::Target {
                    index: 0,
                    current: false
                },
                Entry::Target {
                    index: 1,
                    current: true
                },
                Entry::Target {
                    index: 2,
                    current: false
                },
                Entry::Note(
                    "Picking one moves only this session; the conversation continues there."
                ),
                Entry::Separator,
                Entry::MoveAll,
                Entry::Separator,
                Entry::AddAccount,
                Entry::CodexAccounts,
            ]
        );
        let terminal = account_menu(&s, None, false, false);
        assert_eq!(terminal.label, "me@example.com");
        assert_eq!(
            terminal.tooltip,
            "This session runs on the main login. Click to switch it."
        );
        assert!(terminal.entries.contains(&Entry::Target {
            index: 0,
            current: true
        }));
        assert!(!terminal.entries.contains(&Entry::CodexAccounts));
        assert_eq!(
            terminal
                .entries
                .iter()
                .filter(|e| matches!(e, Entry::Target { .. }))
                .count(),
            1
        );
        // One target only: no Move All.
        let alone = account_menu(&Snapshot::default(), None, true, false);
        assert!(!alone.entries.contains(&Entry::MoveAll));
    }
}
