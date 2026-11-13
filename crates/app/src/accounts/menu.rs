use super::store::Snapshot;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    Main,
    Lane(String),
    Saved(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub title: String,
    pub place: Place,
    pub account: Option<String>,
}

pub const LANE_ACCOUNT_UNKNOWN: &str =
    "That lane's account isn't known yet, so its sessions can't be moved there. Open Accounts to check it.";

pub fn move_all_account(target: &Target) -> Result<Option<String>, &'static str> {
    match &target.place {
        Place::Main => Ok(None),
        Place::Lane(_) | Place::Saved(_) => {
            target.account.clone().map(Some).ok_or(LANE_ACCOUNT_UNKNOWN)
        }
    }
}

pub fn move_needs_accounts(target: &Target) -> bool {
    matches!(target.place, Place::Saved(_))
}

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Header(&'static str),
    Target { index: usize, current: bool },
    Note(&'static str),
    Separator,
    MoveAll,
    AddAccount,
    CodexAccounts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountMenu {
    pub label: String,
    pub lane: Option<String>,
    pub tooltip: String,
    pub targets: Vec<Target>,
    pub entries: Vec<Entry>,
}

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
    fn move_all_never_sends_the_main_login_for_a_lane() {
        let t = targets(&snapshot());
        assert_eq!(move_all_account(&t[0]), Ok(None));
        assert_eq!(move_all_account(&t[1]), Ok(Some("a2".into())));
        assert_eq!(move_all_account(&t[2]), Ok(Some("a3".into())));
        let unknown = Target {
            title: "night".into(),
            place: Place::Lane("/h/.claude-lanes/night".into()),
            account: None,
        };
        assert_eq!(move_all_account(&unknown), Err(LANE_ACCOUNT_UNKNOWN));
        let needs: Vec<bool> = t.iter().map(move_needs_accounts).collect();
        assert_eq!(
            needs,
            [false, false, true],
            "only a saved account needs a lane made"
        );
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
        let alone = account_menu(&Snapshot::default(), None, true, false);
        assert!(!alone.entries.contains(&Entry::MoveAll));
    }
}
