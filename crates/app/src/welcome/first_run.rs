use super::defaults::Imported;
use super::facts::{HookFacts, SwiftApp};

pub const TITLE: &str = "Welcome to Notchling";

pub const OWNERSHIP: &str =
    "Until each area is handed over, the Swift Notchling owns it. This app only watches.";

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookBlock {
    Unknown(String),
    SwiftOwns {
        entries: usize,
    },
    Refused(String),
    Installed,
    Offer {
        changes: Vec<String>,
        notes: Vec<String>,
        install_on: bool,
        off_message: Option<String>,
    },
    Asked {
        applied: bool,
        message: Option<String>,
        changes: Vec<String>,
        backup: Option<String>,
    },
}

impl HookBlock {
    pub fn offers_install(&self) -> bool {
        matches!(self, HookBlock::Offer { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirstRun {
    pub swift: SwiftApp,
    pub hook: Result<HookFacts, String>,
    pub asked: Option<Result<HookFacts, String>>,
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

    pub fn install_button(&self) -> &'static str {
        match self.hook_block() {
            HookBlock::Offer {
                install_on: true, ..
            } => "Install Hook",
            _ => "Show What Install Would Do",
        }
    }

    pub fn import_line(&self) -> String {
        match &self.imported {
            None => "Reads the Swift app's preferences and copies them here. The Swift app's own are never changed.".into(),
            Some(Ok(done)) => done.line(),
            Some(Err(why)) => format!("Couldn't bring settings over: {why}"),
        }
    }

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
