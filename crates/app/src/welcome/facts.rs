use std::path::PathBuf;

use serde_json::Value;

pub const SWIFT_BUNDLE_ID: &str = "local.notchling";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwiftApp {
    Running,
    Installed,
    NotFound,
}

impl SwiftApp {
    pub fn present(self) -> bool {
        self != SwiftApp::NotFound
    }

    pub fn line(self) -> &'static str {
        match self {
            SwiftApp::Running => "The Swift Notchling is running on this Mac.",
            SwiftApp::Installed => "The Swift Notchling is installed on this Mac, and not running.",
            SwiftApp::NotFound => "No Swift Notchling was found on this Mac.",
        }
    }
}

pub trait SwiftProbe: Send + Sync {
    fn find(&self) -> SwiftApp;
}

pub struct SystemProbe {
    pub applications: PathBuf,
}

impl Default for SystemProbe {
    fn default() -> Self {
        SystemProbe {
            applications: PathBuf::from("/Applications"),
        }
    }
}

impl SwiftProbe for SystemProbe {
    fn find(&self) -> SwiftApp {
        if notchling_platform::apps::is_running(SWIFT_BUNDLE_ID) {
            SwiftApp::Running
        } else if is_swift_bundle(&self.applications.join("Notchling.app")) {
            SwiftApp::Installed
        } else {
            SwiftApp::NotFound
        }
    }
}

fn is_swift_bundle(app: &std::path::Path) -> bool {
    std::fs::read_to_string(app.join("Contents/Info.plist"))
        .is_ok_and(|plist| plist.contains(&format!("<string>{SWIFT_BUNDLE_ID}</string>")))
}

pub struct FixedProbe(pub SwiftApp);

impl SwiftProbe for FixedProbe {
    fn find(&self) -> SwiftApp {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillChange {
    pub name: String,
    pub tool: String,
    pub action: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HookFacts {
    pub install_on: bool,
    pub applied: bool,
    pub message: Option<String>,
    pub state: String,
    pub refused: Option<String>,
    pub swift_entries: Vec<String>,
    pub our_entries: usize,
    pub changes: Vec<String>,
    pub notes: Vec<String>,
    pub blocked: Option<String>,
    pub skills: Vec<SkillChange>,
    pub backup: Option<String>,
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

impl HookFacts {
    pub fn parse(reply: &Value) -> Option<HookFacts> {
        let plan = reply.get("plan")?;
        let hooks = plan.get("hooks")?;
        let present = hooks.get("present").and_then(Value::as_array);
        fn origin<'a>(
            present: Option<&'a Vec<Value>>,
            o: &'a str,
        ) -> impl Iterator<Item = &'a Value> {
            present
                .into_iter()
                .flatten()
                .filter(move |f| f.get("origin").and_then(Value::as_str) == Some(o))
        }
        let skills = plan
            .get("skills")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|s| {
                Some(SkillChange {
                    name: s.get("name")?.as_str()?.to_owned(),
                    tool: s.get("tool")?.as_str()?.to_owned(),
                    action: s.get("action")?.as_str()?.to_owned(),
                })
            })
            .collect();
        Some(HookFacts {
            install_on: reply.get("on").and_then(Value::as_bool).unwrap_or(false),
            applied: reply
                .get("applied")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            message: reply
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned),
            state: hooks
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or("missing")
                .to_owned(),
            refused: hooks
                .get("refused")
                .and_then(Value::as_str)
                .map(str::to_owned),
            swift_entries: origin(present, "swift")
                .filter_map(|f| f.get("command").and_then(Value::as_str))
                .map(str::to_owned)
                .collect(),
            our_entries: origin(present, "notchling").count(),
            changes: strings(hooks.get("changes").unwrap_or(&Value::Null)),
            notes: strings(hooks.get("notes").unwrap_or(&Value::Null)),
            blocked: plan
                .get("blocked")
                .and_then(Value::as_str)
                .map(str::to_owned),
            skills,
            backup: reply
                .get("backup")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    }

    pub fn swift_owns_hook(&self) -> bool {
        !self.swift_entries.is_empty()
    }

    pub fn we_own_hook(&self) -> bool {
        self.our_entries > 0 && self.swift_entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reply(present: Value, changes: Value, on: bool) -> Value {
        json!({"on": on, "applied": false,
            "message": "install is off: the Swift app owns hooks and skills until the maintainer turns this on",
            "plan": {"action": "install", "changed": true, "blocked": null,
              "hooks": {"file": "/h/.claude/settings.json", "state": "present", "read_only": false,
                "refused": null, "changed": true, "changes": changes,
                "notes": ["the Swift app's hook is in PermissionRequest (*); it was not changed"],
                "present": present},
              "skills": [{"name": "notchling-pages", "tool": "claude", "path": "/x",
                          "action": "write", "reason": null, "error": null}]}})
    }

    #[test]
    fn a_swift_hook_in_the_file_is_read_as_theirs() {
        let facts = HookFacts::parse(&reply(
            json!([{"event": "PermissionRequest", "matcher": "*", "origin": "swift",
                    "command": "'/Applications/Notchling.app/Contents/MacOS/Notchling' --hook",
                    "managed": false}]),
            json!(["add PreToolUse (AskUserQuestion): 'x' hook (timeout 300 s)"]),
            false,
        ))
        .unwrap();
        assert!(facts.swift_owns_hook() && !facts.we_own_hook());
        assert_eq!(facts.swift_entries.len(), 1);
        assert!(!facts.install_on && !facts.applied);
        assert!(facts
            .message
            .as_deref()
            .unwrap()
            .starts_with("install is off"));
        assert_eq!(facts.changes.len(), 1);
        assert_eq!(facts.skills[0].name, "notchling-pages");
        assert_eq!(facts.state, "present");
    }

    #[test]
    fn no_swift_hook_means_an_install_may_be_offered() {
        let facts = HookFacts::parse(&reply(json!([]), json!(["add a"]), true)).unwrap();
        assert!(!facts.swift_owns_hook() && !facts.we_own_hook());
        assert!(facts.install_on);
        let ours = HookFacts::parse(&reply(
            json!([{"event": "PreToolUse", "matcher": "Artifact", "origin": "notchling",
                    "command": "'/b/notchling' hook", "managed": true}]),
            json!([]),
            true,
        ))
        .unwrap();
        assert!(ours.we_own_hook());
    }

    #[test]
    fn a_reply_that_is_not_a_plan_is_none() {
        assert_eq!(HookFacts::parse(&json!({"version": "0.1"})), None);
        assert_eq!(HookFacts::parse(&Value::Null), None);
        assert_eq!(HookFacts::parse(&json!({"plan": {}})), None);
    }

    #[test]
    fn a_refused_file_says_why() {
        let facts = HookFacts::parse(&json!({"on": false, "applied": false, "plan": {
            "blocked": null, "skills": [],
            "hooks": {"state": "refused", "refused": "settings.json isn't valid JSON",
                      "changes": [], "notes": [], "present": []}}}))
        .unwrap();
        assert_eq!(facts.state, "refused");
        assert_eq!(
            facts.refused.as_deref(),
            Some("settings.json isn't valid JSON")
        );
    }

    #[test]
    fn the_probe_wording() {
        assert!(SwiftApp::Running.present() && SwiftApp::Installed.present());
        assert!(!SwiftApp::NotFound.present());
        assert!(SwiftApp::Installed.line().contains("not running"));
        assert_eq!(FixedProbe(SwiftApp::Running).find(), SwiftApp::Running);
    }

    #[test]
    fn the_system_probe_looks_in_the_folder_it_is_given() {
        let dir = std::env::temp_dir().join(format!("nl-probe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let plist = |folder: &str, id: &str| {
            let contents = dir.join(folder).join("Notchling.app/Contents");
            std::fs::create_dir_all(&contents).unwrap();
            std::fs::write(
                contents.join("Info.plist"),
                format!("<dict>\n  <key>CFBundleIdentifier</key><string>{id}</string>\n</dict>\n"),
            )
            .unwrap();
            SystemProbe {
                applications: dir.join(folder),
            }
        };
        assert_ne!(plist("swift", SWIFT_BUNDLE_ID).find(), SwiftApp::NotFound);
        assert!(matches!(
            plist("next", "local.notchling.next").find(),
            SwiftApp::NotFound | SwiftApp::Running
        ));
        let empty = SystemProbe {
            applications: dir.join("nowhere"),
        };
        assert!(matches!(
            empty.find(),
            SwiftApp::NotFound | SwiftApp::Running
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
