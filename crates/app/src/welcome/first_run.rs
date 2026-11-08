//! The first-run view's model: what the app found, what it says about who owns what, and the two
//! things it offers (bringing over the Swift app's settings, and the hook). Pure; the view draws it
//! and the window runs the calls.

use super::defaults::Imported;
use super::facts::{HookFacts, SwiftApp};

pub const TITLE: &str = "Welcome to Notchling";

/// The plain statement of who owns what, until the maintainer hands each area over (decision 0011).
pub const OWNERSHIP: &str =
    "Until each area is handed over, the Swift Notchling owns it. This app only watches.";

/// What the Swift app owns, one line each.
pub const OWNED_BY_SWIFT: [(&str, &str); 5] = [
    (
        "The hook",
        "its entries in Claude Code's settings.json answer permission and question prompts",
    ),
    ("The Secret Vault", "its Keychain item and vault.sock"),
    ("Logins", "account switching, lanes and limit watching"),
    (
        "The phone bridge",
        "ports 47821 and 47822, and the paired phones",
    ),
    ("Sessions", "replies, answers, stop and new sessions"),
];

/// What the hook part of the view shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookBlock {
    /// Couldn't be checked: no daemon, or it refused.
    Unknown(String),
    /// The Swift app's `--hook` is in the file: it stays, and no install is offered.
    SwiftOwns { entries: usize },
    /// settings.json can't be edited.
    Refused(String),
    /// This app's hook is already there and the Swift app's is not.
    Installed,
    /// Nothing answers prompts yet: an install may be offered.
    Offer {
        changes: Vec<String>,
        notes: Vec<String>,
        /// The daemon's `install` switch.
        install_on: bool,
        /// What the daemon says while it is off.
        off_message: Option<String>,
    },
    /// An install was asked for. `applied` is false for a dry run.
    Asked {
        applied: bool,
        message: Option<String>,
        changes: Vec<String>,
        backup: Option<String>,
    },
}

impl HookBlock {
    /// True only for [`HookBlock::Offer`], the one state with an install button.
    pub fn offers_install(&self) -> bool {
        matches!(self, HookBlock::Offer { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirstRun {
    pub swift: SwiftApp,
    /// `install.plan`, or why it couldn't be had.
    pub hook: Result<HookFacts, String>,
    /// After the install button: the daemon's answer.
    pub asked: Option<Result<HookFacts, String>>,
    /// After the import button.
    pub imported: Option<Result<Imported, String>>,
}

impl FirstRun {
    pub fn new(swift: SwiftApp, hook: Result<HookFacts, String>) -> FirstRun {
        FirstRun {
            swift,
            hook,
            asked: None,
            imported: None,
        }
    }

    /// The hook part, from what is known now.
    pub fn hook_block(&self) -> HookBlock {
        if let Some(asked) = &self.asked {
            return match asked {
                Ok(f) => HookBlock::Asked {
                    applied: f.applied,
                    message: f.message.clone(),
                    changes: f.changes.clone(),
                    backup: f.backup.clone(),
                },
                Err(why) => HookBlock::Unknown(why.clone()),
            };
        }
        let facts = match &self.hook {
            Ok(f) => f,
            Err(why) => return HookBlock::Unknown(why.clone()),
        };
        if let Some(why) = &facts.refused {
            return HookBlock::Refused(why.clone());
        }
        if facts.swift_owns_hook() {
            return HookBlock::SwiftOwns {
                entries: facts.swift_entries.len(),
            };
        }
        if facts.we_own_hook() && facts.changes.is_empty() {
            return HookBlock::Installed;
        }
        HookBlock::Offer {
            changes: facts.changes.clone(),
            notes: facts.notes.clone(),
            install_on: facts.install_on,
            off_message: facts.message.clone(),
        }
    }

    /// The words on the install button.
    pub fn install_button(&self) -> &'static str {
        match self.hook_block() {
            HookBlock::Offer {
                install_on: true, ..
            } => "Install Hook",
            _ => "Show What Install Would Do",
        }
    }

    /// The line under the settings button.
    pub fn import_line(&self) -> String {
        match &self.imported {
            None => "Reads the Swift app's preferences and copies them here. The Swift app's own are never changed.".into(),
            Some(Ok(done)) => done.line(),
            Some(Err(why)) => format!("Couldn't bring settings over: {why}"),
        }
    }

    /// The line about the Swift app, plus what it means for this run.
    pub fn swift_line(&self) -> String {
        self.swift.line().to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn facts(present: serde_json::Value, changes: serde_json::Value, on: bool) -> HookFacts {
        HookFacts::parse(&json!({"on": on, "applied": false,
            "message": if on { serde_json::Value::Null } else { json!("install is off: …") },
            "plan": {"blocked": null, "skills": [],
              "hooks": {"state": "present", "refused": null, "changes": changes,
                        "notes": ["n"], "present": present}}}))
        .unwrap()
    }

    fn swift_entry() -> serde_json::Value {
        json!([{"event": "PermissionRequest", "matcher": "*", "origin": "swift",
                "command": "'/A/Notchling' --hook", "managed": false}])
    }

    fn run(facts: HookFacts) -> FirstRun {
        FirstRun::new(SwiftApp::Running, Ok(facts))
    }

    #[test]
    fn a_swift_hook_means_no_install_is_offered() {
        let f = run(facts(swift_entry(), json!(["add x"]), true));
        assert_eq!(f.hook_block(), HookBlock::SwiftOwns { entries: 1 });
        assert!(!f.hook_block().offers_install());
    }

    #[test]
    fn with_no_swift_hook_an_install_is_offered_and_off_is_said_plainly() {
        let f = run(facts(
            json!([]),
            json!(["add PreToolUse (AskUserQuestion)"]),
            false,
        ));
        let block = f.hook_block();
        assert!(block.offers_install());
        let HookBlock::Offer {
            changes,
            install_on,
            off_message,
            notes,
        } = block
        else {
            panic!("offer");
        };
        assert_eq!((changes.len(), install_on, notes.len()), (1, false, 1));
        assert_eq!(off_message.as_deref(), Some("install is off: …"));
        assert_eq!(f.install_button(), "Show What Install Would Do");
        let on = run(facts(json!([]), json!(["add x"]), true));
        assert_eq!(on.install_button(), "Install Hook");
    }

    #[test]
    fn our_own_hook_already_in_place_is_installed() {
        let ours = json!([{"event": "PreToolUse", "matcher": "Artifact", "origin": "notchling",
                           "command": "'/b/notchling' hook", "managed": true}]);
        assert_eq!(
            run(facts(ours, json!([]), true)).hook_block(),
            HookBlock::Installed
        );
    }

    #[test]
    fn a_refused_file_and_a_missing_daemon_are_said() {
        let refused = HookFacts::parse(&json!({"on": false, "applied": false, "plan": {
            "blocked": null, "skills": [],
            "hooks": {"state": "refused", "refused": "not valid JSON", "changes": [],
                      "notes": [], "present": []}}}))
        .unwrap();
        assert_eq!(
            run(refused).hook_block(),
            HookBlock::Refused("not valid JSON".into())
        );
        let none = FirstRun::new(SwiftApp::NotFound, Err("daemon not attached".into()));
        assert_eq!(
            none.hook_block(),
            HookBlock::Unknown("daemon not attached".into())
        );
        assert!(!none.hook_block().offers_install());
    }

    #[test]
    fn asking_shows_the_dry_run_and_its_message() {
        let mut f = run(facts(json!([]), json!(["add x"]), false));
        let answer = facts(json!([]), json!(["add x"]), false);
        f.asked = Some(Ok(answer));
        let HookBlock::Asked {
            applied,
            message,
            changes,
            ..
        } = f.hook_block()
        else {
            panic!("asked");
        };
        assert!(!applied && changes.len() == 1);
        assert!(message.unwrap().starts_with("install is off"));
        f.asked = Some(Err("settings.json changed meanwhile".into()));
        assert_eq!(
            f.hook_block(),
            HookBlock::Unknown("settings.json changed meanwhile".into())
        );
    }

    #[test]
    fn the_import_line_follows_what_happened() {
        let mut f = run(facts(json!([]), json!([]), true));
        assert!(f.import_line().contains("never changed"));
        f.imported = Some(Ok(Imported {
            added: vec!["mode".into()],
            ..Imported::default()
        }));
        assert_eq!(f.import_line(), "Brought over 1 setting.");
        f.imported = Some(Err("unreadable".into()));
        assert!(f.import_line().contains("unreadable"));
    }

    #[test]
    fn the_ownership_statement_names_every_area() {
        let text: String = OWNED_BY_SWIFT
            .iter()
            .map(|(a, _)| *a)
            .collect::<Vec<_>>()
            .join(" ");
        for word in ["hook", "Vault", "Logins", "phone bridge", "Sessions"] {
            assert!(text.to_lowercase().contains(&word.to_lowercase()), "{word}");
        }
        assert!(OWNERSHIP.contains("Swift"));
    }
}
