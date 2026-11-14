use serde_json::{json, Value};

use super::agents::Page;
use super::client_model::{client_part, client_part_with, ClientPart, Extras};
use super::hook_rows::HooksShown;
use crate::acting::Acting;
use crate::notch::source::{fixture_clock, DataRoot, Source};
use crate::views::ViewSpec;
use crate::welcome::facts::HookFacts;
use notchling_engine::versions::Tool;

use super::banner::{Banner, Banners};
use super::new_session::{accounts, folders, NewSession};
use crate::accounts::menu::targets;
use crate::accounts::renders::{snapshot, Shown as AccountsShown};

pub const VIEWS: [ViewSpec; 12] = [
    ViewSpec {
        name: "banner",
        size: (620.0, 260.0),
        about: "banners: limit switches, an error and a notice (fixture data)",
    },
    ViewSpec {
        name: "new-session",
        size: (680.0, 460.0),
        about: "the New Session sheet, acting off: Start disabled (fixture data)",
    },
    ViewSpec {
        name: "trust-folder",
        size: (680.0, 460.0),
        about: "the Trust Folder pop-up over New Session, acting on (fixture data)",
    },
    ViewSpec {
        name: "settings",
        size: (620.0, 1660.0),
        about: "the Settings sheet's General page with acting shown read-only (defaults), the Search section and the Chat section",
    },
    ViewSpec {
        name: "settings-agents",
        size: (620.0, 760.0),
        about: "the Settings sheet's Agents page: Claude Code's Hooks with all five installed (fixture reply)",
    },
    ViewSpec {
        name: "settings-agents-partial",
        size: (620.0, 760.0),
        about: "the Agents page's Hooks with a Swift app entry, an unmanaged one and some missing (fixture reply)",
    },
    ViewSpec {
        name: "settings-agents-empty",
        size: (620.0, 760.0),
        about: "the Agents page's Hooks with none installed (fixture reply)",
    },
    ViewSpec {
        name: "settings-agents-detached",
        size: (620.0, 760.0),
        about: "the Agents page's Hooks with no daemon attached: the five explained, none checked",
    },
    ViewSpec {
        name: "settings-capabilities",
        size: (620.0, 3500.0),
        about: "the Agents page with What Notchling Can Do: each tool's fallback, not applicable and not built rows",
    },
    ViewSpec {
        name: "settings-agents-client",
        size: (620.0, 900.0),
        about: "the Agents page's Client part: the Mac's Claude Code and two workers, one the same and one behind (fixture reply)",
    },
    ViewSpec {
        name: "settings-agents-client-failed",
        size: (620.0, 900.0),
        about: "the Client part after a new version failed its check: the red line, the soak and Go Back",
    },
    ViewSpec {
        name: "settings-agents-client-update",
        size: (620.0, 900.0),
        about: "the Client part with a newer version out: Latest, Update (Install off, so disabled with its note) and the Partly Checked line",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Banner,
    NewSession,
    TrustFolder,
    Settings,
    SettingsAgents,
    SettingsAgentsPartial,
    SettingsAgentsEmpty,
    SettingsAgentsDetached,
    SettingsCapabilities,
    SettingsAgentsClient,
    SettingsAgentsClientFailed,
    SettingsAgentsClientUpdate,
}

pub fn shown(name: &str) -> Option<Shown> {
    Some(match name {
        "banner" => Shown::Banner,
        "new-session" => Shown::NewSession,
        "trust-folder" => Shown::TrustFolder,
        "settings" => Shown::Settings,
        "settings-agents" => Shown::SettingsAgents,
        "settings-agents-partial" => Shown::SettingsAgentsPartial,
        "settings-agents-empty" => Shown::SettingsAgentsEmpty,
        "settings-agents-detached" => Shown::SettingsAgentsDetached,
        "settings-capabilities" => Shown::SettingsCapabilities,
        "settings-agents-client" => Shown::SettingsAgentsClient,
        "settings-agents-client-failed" => Shown::SettingsAgentsClientFailed,
        "settings-agents-client-update" => Shown::SettingsAgentsClientUpdate,
        _ => return None,
    })
}

pub fn acting(shown: Shown) -> Acting {
    match shown {
        Shown::TrustFolder => Acting::On,
        Shown::Banner
        | Shown::NewSession
        | Shown::Settings
        | Shown::SettingsAgents
        | Shown::SettingsAgentsPartial
        | Shown::SettingsAgentsEmpty
        | Shown::SettingsAgentsDetached
        | Shown::SettingsCapabilities
        | Shown::SettingsAgentsClient
        | Shown::SettingsAgentsClientFailed
        | Shown::SettingsAgentsClientUpdate => Acting::Off,
    }
}

pub fn page(shown: Shown) -> Page {
    match shown {
        Shown::SettingsAgents
        | Shown::SettingsAgentsPartial
        | Shown::SettingsAgentsEmpty
        | Shown::SettingsAgentsDetached
        | Shown::SettingsCapabilities
        | Shown::SettingsAgentsClient
        | Shown::SettingsAgentsClientFailed
        | Shown::SettingsAgentsClientUpdate => Page::Agents,
        _ => Page::General,
    }
}

fn hook(event: &str, matcher: &str, origin: &str, managed: bool) -> Value {
    json!({"event": event, "matcher": matcher, "origin": origin,
        "command": "'/Applications/Notchling.app/Contents/MacOS/notchling' hook",
        "managed": managed})
}

pub fn hooks(shown: Shown) -> HooksShown {
    let present = match shown {
        Shown::SettingsAgentsDetached => return HooksShown::Detached,
        Shown::SettingsAgentsEmpty => vec![],
        Shown::SettingsAgentsPartial => vec![
            hook("PermissionRequest", "*", "swift", false),
            hook("PreToolUse", "AskUserQuestion", "notchling", true),
            hook("PostToolUse", "Artifact", "notchling", true),
            hook("SessionStart", "*", "swift", false),
        ],
        _ => vec![
            hook("PermissionRequest", "*", "notchling", true),
            hook("PreToolUse", "AskUserQuestion", "notchling", true),
            hook("PreToolUse", "Artifact", "notchling", true),
            hook("PostToolUse", "Artifact", "notchling", true),
            hook("PostToolUseFailure", "Artifact", "notchling", true),
        ],
    };
    let reply = json!({"on": false, "applied": false,
        "plan": {"blocked": null, "skills": [],
            "hooks": {"state": "present", "refused": null, "changes": [], "notes": [],
                      "present": present}}});
    HooksShown::from_plan(HookFacts::parse(&reply).ok_or_else(|| "no plan".to_owned()))
}

pub fn client(shown: Shown) -> Option<ClientPart> {
    match shown {
        Shown::SettingsAgentsDetached => Some(ClientPart::not_running()),
        Shown::SettingsAgents
        | Shown::SettingsAgentsPartial
        | Shown::SettingsAgentsEmpty
        | Shown::SettingsAgentsClient
        | Shown::SettingsAgentsClientFailed
        | Shown::SettingsAgentsClientUpdate => {
            let mut row = json!({"tool": "claude", "installed": {"version": "2.1.324", "sha256": null},
                 "method": "native", "path": "/Users/me/.local/bin/claude"});
            let mut extras = Extras::default();
            let reference = json!({"version": "2.1.324", "sha256": "ab", "result": "partly",
                "partly": true, "at": 1_700_000_000_000_i64,
                "unchecked": ["screens", "dialogs", "turns"]});
            if shown == Shown::SettingsAgentsClientUpdate {
                row["latest"] = json!("2.1.325");
                row["reference"] = reference.clone();
                row["usable"] = json!(true);
                row["soak"] = json!({"since": "2.1.324", "turns": 41, "unknown": 0,
                    "line": "41 turns on real sessions, none unknown"});
            }
            if shown == Shown::SettingsAgentsClientFailed {
                row["installed"] = json!({"version": "2.1.325", "sha256": null});
                row["reference"] = reference;
                row["failed"] = json!([{"version": "2.1.325", "part": "transcripts",
                    "at": 1_700_000_300_000_i64, "why": "2 lines it can't read"}]);
                row["goBack"] = json!("2.1.324");
                row["soak"] = json!({"since": "2.1.325", "turns": 12, "unknown": 2,
                    "line": "12 turns on real sessions, 2 unknown lines"});
            }
            let get = json!({"tools": [row]});
            let workers = json!({"workers": {
                "m1": {"tools": {"claude": {"state": "installed", "version": "2.1.324",
                    "method": "native", "path": "/home/w/.local/bin/claude"}}, "atMs": 1},
                "vps-sing-1": {"tools": {"claude": {"state": "installed", "version": "2.1.320",
                    "method": "native", "path": "/home/w/.local/bin/claude"}}, "atMs": 1},
                "ub-3": {"tools": {"claude": {"state": "notInstalled"}}, "atMs": 1},
            }});
            extras.install_on = false;
            Some(
                if shown == Shown::SettingsAgentsClientUpdate
                    || shown == Shown::SettingsAgentsClientFailed
                {
                    client_part_with(Tool::Claude, &get, &workers, "/Users/me", extras)
                } else {
                    client_part(Tool::Claude, &get, &workers, "/Users/me")
                },
            )
        }
        _ => None,
    }
}

pub fn new_session(shown: Shown) -> NewSession {
    let mut source = Source::keeping_everything(DataRoot::for_render());
    let agents = source.load(0).agents;
    let saved = vec!["/work/notchling/apps/flutter".to_owned()];
    let made_up = snapshot(AccountsShown::Claude);
    let mut sheet = NewSession::new(
        accounts(&targets(&made_up), &agents, &made_up.providers.providers),
        folders(&saved, &agents),
        "",
    );
    if shown == Shown::TrustFolder {
        sheet.pick_folder(0);
        sheet.start();
        sheet.started(Err("Workspace not trusted".into()));
    }
    sheet
}

pub fn banners() -> Banners {
    let mut source = Source::keeping_everything(DataRoot::for_render());
    let now = fixture_clock(&source.load(0).agents);
    let mut b = Banners::default();
    b.push(Banner::notice(
        "Notchling",
        "Notifications are off for this build, so notices show here.",
        now,
    ));
    b.push(Banner::error(
        "Couldn't stop the session: acting is off.",
        now,
    ));
    b.push(Banner::lane_switched(
        "night",
        "ops@example.com",
        "dev@example.com",
        now,
    ));
    b.push(Banner::switched(
        "me@example.com",
        "team@example.com",
        12.4,
        now,
    ));
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sheets::new_session::Stage;

    #[test]
    fn every_view_has_its_data() {
        for spec in VIEWS {
            assert!(shown(spec.name).is_some(), "{}", spec.name);
        }
        assert_eq!(shown("chat"), None);
        let sheet = new_session(Shown::NewSession);
        assert!(!sheet.accounts.is_empty() && sheet.folders.len() >= 2);
        assert_eq!(sheet.stage, Stage::Choosing);
        let trust = new_session(Shown::TrustFolder);
        assert_eq!(trust.stage, Stage::Untrusted);
        assert_eq!(acting(Shown::TrustFolder), Acting::On);
        assert_eq!(banners().all().len(), 4);
        assert_eq!(hooks(Shown::SettingsAgents).lines().len(), 5);
        assert_eq!(hooks(Shown::SettingsAgentsPartial).lines().len(), 6);
        assert_eq!(hooks(Shown::SettingsAgentsEmpty).lines().len(), 5);
        assert_eq!(hooks(Shown::SettingsAgentsDetached), HooksShown::Detached);
        assert_eq!(page(Shown::Settings), Page::General);
        assert_eq!(page(Shown::SettingsAgents), Page::Agents);
        assert_eq!(
            client(Shown::SettingsAgentsClient).unwrap().workers.len(),
            3
        );
        assert_eq!(client(Shown::Settings), None);
    }
}
