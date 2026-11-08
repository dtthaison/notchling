//! The flows around the account rows, pure: each provider's add panel in its phases (the Swift
//! `SignInPanel` and the pages' add panels), the Remove alert with each provider's words, and the
//! Move All alert (`MoveAll.swift`).

use crate::acting::Control;
use crate::switches::{Switch, Switches};

use super::model::{ANTIGRAVITY, CLAUDE_CODE, CLAUDE_DESKTOP, CODEX};
use super::store::Snapshot;
use super::wire::MovePlan;

/// The add panel under a provider's list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddPanel {
    /// Nothing under way: a line of help and the Add Account button.
    Idle {
        note: &'static str,
        add: Control,
    },
    /// A sign-in is open in the browser or the other app.
    Waiting {
        line: &'static str,
        note: &'static str,
        /// The sign-in page, for Open Again / Open Page and Copy Link.
        url: Option<String>,
        /// Whether Copy Link shows (Claude Code's browser flow).
        copy_link: bool,
    },
    Saving,
    /// A Claude Code sign-in ended well.
    Added {
        line: String,
    },
    /// A Claude Code sign-in failed; Try Again and Cancel.
    Failed {
        message: String,
    },
}

/// The idle line per provider.
fn idle_note(provider: &str) -> &'static str {
    match provider {
        CLAUDE_CODE => "Sign in to another Claude account to switch to it from here.",
        CLAUDE_DESKTOP => "Claude Desktop opens signed out so you can sign in to another account.",
        CODEX => {
            "Signs in with codex login in your browser. Codex keeps using the current account."
        }
        ANTIGRAVITY => "Antigravity opens signed out so you can sign in to another Google account.",
        _ => "",
    }
}

/// The waiting lines per provider.
fn waiting_lines(provider: &str) -> (&'static str, &'static str) {
    match provider {
        CLAUDE_CODE => (
            "Sign in with the account to add in your browser…",
            "Browser already signed in to a different account? Copy the link into a private window.",
        ),
        CLAUDE_DESKTOP => (
            "Sign in to the other account in Claude Desktop…",
            "Notchling saves it as soon as you're signed in.",
        ),
        CODEX => (
            "Sign in to the other account in your browser…",
            "Codex keeps using the current account.",
        ),
        ANTIGRAVITY => (
            "Sign in to the other account in Antigravity…",
            "Notchling saves it as soon as you're signed in.",
        ),
        _ => ("Signing in…", ""),
    }
}

/// The panel for `provider` from the snapshot. `busy` is a click on its way.
pub fn add_panel(provider: &str, s: &Snapshot, switches: &Switches, busy: bool) -> AddPanel {
    let control = switches.control(Switch::Accounts);
    let idle = |disabled: bool| AddPanel::Idle {
        note: idle_note(provider),
        add: if (busy || disabled) && control.enabled {
            Control {
                enabled: false,
                tooltip: None,
            }
        } else {
            control
        },
    };
    if provider == CLAUDE_CODE {
        let Some(add) = &s.add else {
            return idle(false);
        };
        return match add.state.as_str() {
            "waiting" => {
                let (line, note) = waiting_lines(provider);
                AddPanel::Waiting {
                    line,
                    note,
                    url: add.url.clone(),
                    copy_link: true,
                }
            }
            "saving" => AddPanel::Saving,
            "added" => AddPanel::Added {
                line: format!("Added {}.", add.email.clone().unwrap_or_default()),
            },
            "failed" => AddPanel::Failed {
                message: add
                    .error
                    .clone()
                    .unwrap_or_else(|| "The sign-in didn't finish.".into()),
            },
            _ => idle(false),
        };
    }
    let Some(p) = s.providers.providers.iter().find(|p| p.id == provider) else {
        return idle(true);
    };
    if p.adding {
        let (line, note) = waiting_lines(provider);
        AddPanel::Waiting {
            line,
            note,
            url: p.sign_in_url.clone(),
            copy_link: false,
        }
    } else {
        idle(p.busy || !p.available)
    }
}

/// An alert over the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    pub title: String,
    pub message: String,
    pub button: String,
    pub destructive: bool,
}

/// The Remove alert's words for `provider`, given whether `email` is the account in use.
pub fn remove_alert(provider: &str, email: &str, in_use: bool) -> Alert {
    let (title, message) = match (provider, in_use) {
        (CLAUDE_DESKTOP, _) => (
            format!("Remove {email} from Desktop list?"),
            "Notchling forgets its saved Desktop session. Claude Desktop stays signed in if it's \
             the account in use; add it again by signing in to it in Desktop."
                .to_owned(),
        ),
        (CODEX, true) => (
            format!("Remove {email}?"),
            "Notchling forgets its saved login. Codex stays signed in to it.".to_owned(),
        ),
        (ANTIGRAVITY, true) => (
            format!("Remove {email}?"),
            "Notchling forgets its saved login. Antigravity stays signed in to it.".to_owned(),
        ),
        (ANTIGRAVITY, false) => (
            format!("Remove {email}?"),
            "Notchling forgets its saved login. Sign in to it in Antigravity to add it again."
                .to_owned(),
        ),
        (_, true) => (
            format!("Remove {email}?"),
            "Notchling forgets its saved login. Claude Code stays signed in to it.".to_owned(),
        ),
        (_, false) => (
            format!("Remove {email}?"),
            "Notchling forgets its saved login. Add it again to use it.".to_owned(),
        ),
    };
    Alert {
        title,
        message,
        button: "Remove".into(),
        destructive: true,
    }
}

/// The Move All alert for a dry run's plan: `email` is the target's.
pub fn move_all_alert(email: &str, plan: &MovePlan) -> Alert {
    let mut parts = Vec::new();
    if plan.jobs > 0 {
        parts.push(if plan.jobs == 1 {
            "1 background session in a lane stops and resumes on the main login; a working one \
             is cut off mid-step."
                .to_owned()
        } else {
            format!(
                "{} background sessions in lanes stop and resume on the main login; a working one \
                 is cut off mid-step.",
                plan.jobs
            )
        });
    }
    if plan.switch_main {
        parts.push(format!(
            "The main login switches to {email}, and its sessions go with it."
        ));
    }
    if let Some(lane) = &plan.close_lane {
        parts.push(format!("The lane holding {email} closes ({lane})."));
        if !plan.signed_out.is_empty() {
            parts.push(format!(
                "Terminal sessions in it are signed out: {}.",
                plan.signed_out.join(", ")
            ));
        }
    }
    parts.push("Each moved session re-reads its whole conversation on its first reply.".to_owned());
    Alert {
        title: format!("Move every session to {email}?"),
        message: parts.join(" "),
        button: "Move All".into(),
        destructive: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::wire::{AddProgress, Provider, ProviderList};
    use crate::switches::ACCOUNTS_OFF;

    #[test]
    fn the_claude_panel_follows_the_sign_in() {
        let on = Switches::all_on();
        let mut s = Snapshot::default();
        assert_eq!(
            add_panel(CLAUDE_CODE, &s, &on, false),
            AddPanel::Idle {
                note: "Sign in to another Claude account to switch to it from here.",
                add: Control {
                    enabled: true,
                    tooltip: None
                }
            }
        );
        let AddPanel::Idle { add, .. } = add_panel(CLAUDE_CODE, &s, &Switches::all_off(), false)
        else {
            panic!()
        };
        assert_eq!(add.tooltip, Some(ACCOUNTS_OFF));
        let AddPanel::Idle { add, .. } = add_panel(CLAUDE_CODE, &s, &on, true) else {
            panic!()
        };
        assert!(
            !add.enabled && add.tooltip.is_none(),
            "busy: no reason to give"
        );
        s.add = Some(AddProgress {
            login: "l1".into(),
            state: "waiting".into(),
            url: Some("https://claude.ai/login".into()),
            email: None,
            error: None,
        });
        assert!(matches!(
            add_panel(CLAUDE_CODE, &s, &on, false),
            AddPanel::Waiting {
                copy_link: true,
                url: Some(_),
                ..
            }
        ));
        if let Some(a) = s.add.as_mut() {
            a.state = "added".into();
            a.email = Some("new@example.com".into());
        }
        assert_eq!(
            add_panel(CLAUDE_CODE, &s, &on, false),
            AddPanel::Added {
                line: "Added new@example.com.".into()
            }
        );
        if let Some(a) = s.add.as_mut() {
            a.state = "failed".into();
            a.error = Some("The browser closed.".into());
        }
        assert_eq!(
            add_panel(CLAUDE_CODE, &s, &on, false),
            AddPanel::Failed {
                message: "The browser closed.".into()
            }
        );
        if let Some(a) = s.add.as_mut() {
            a.state = "cancelled".into();
        }
        assert!(matches!(
            add_panel(CLAUDE_CODE, &s, &on, false),
            AddPanel::Idle { .. }
        ));
    }

    #[test]
    fn a_providers_panel_follows_its_state() {
        let on = Switches::all_on();
        let mut s = Snapshot::default();
        let AddPanel::Idle { add, .. } = add_panel(CODEX, &s, &on, false) else {
            panic!()
        };
        assert!(!add.enabled, "a provider the daemon doesn't list can't add");
        s.providers = ProviderList {
            accounts_on: true,
            providers: vec![Provider {
                id: CODEX.into(),
                available: true,
                adding: true,
                sign_in_url: Some("https://auth.openai.com".into()),
                ..Provider::default()
            }],
            syncing: false,
        };
        assert_eq!(
            add_panel(CODEX, &s, &on, false),
            AddPanel::Waiting {
                line: "Sign in to the other account in your browser…",
                note: "Codex keeps using the current account.",
                url: Some("https://auth.openai.com".into()),
                copy_link: false,
            }
        );
        s.providers.providers[0].adding = false;
        s.providers.providers[0].busy = true;
        let AddPanel::Idle { add, note } = add_panel(CODEX, &s, &on, false) else {
            panic!()
        };
        assert!(!add.enabled);
        assert!(note.starts_with("Signs in with codex login"));
    }

    #[test]
    fn remove_and_move_all_say_the_swift_words() {
        let a = remove_alert(CLAUDE_CODE, "me@example.com", true);
        assert_eq!(a.title, "Remove me@example.com?");
        assert!(a.message.ends_with("Claude Code stays signed in to it."));
        assert!(a.destructive && a.button == "Remove");
        assert!(remove_alert(CLAUDE_CODE, "x", false)
            .message
            .ends_with("Add it again to use it."));
        assert_eq!(
            remove_alert(CLAUDE_DESKTOP, "x@y", false).title,
            "Remove x@y from Desktop list?"
        );
        assert!(remove_alert(ANTIGRAVITY, "x", false)
            .message
            .contains("Sign in to it in Antigravity"));
        assert!(remove_alert(CODEX, "x", true)
            .message
            .contains("Codex stays signed in"));

        let plan = MovePlan {
            jobs: 2,
            close_lane: Some("night".into()),
            switch_main: true,
            signed_out: vec!["t1".into(), "t2".into()],
        };
        let m = move_all_alert("ops@example.com", &plan);
        assert_eq!(m.title, "Move every session to ops@example.com?");
        assert_eq!(
            m.message,
            "2 background sessions in lanes stop and resume on the main login; a working one is \
             cut off mid-step. The main login switches to ops@example.com, and its sessions go \
             with it. The lane holding ops@example.com closes (night). Terminal sessions in it \
             are signed out: t1, t2. Each moved session re-reads its whole conversation on its \
             first reply."
        );
        assert_eq!(m.button, "Move All");
        let one = move_all_alert(
            "x",
            &MovePlan {
                jobs: 1,
                ..MovePlan::default()
            },
        );
        assert!(one
            .message
            .starts_with("1 background session in a lane stops"));
    }
}
