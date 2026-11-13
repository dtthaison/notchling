use serde_json::{json, Value};

use super::hook_rows::HooksShown;
use crate::acting::Acting;
use crate::notch::source::{fixture_clock, DataRoot, Source};
use crate::views::ViewSpec;
use crate::welcome::facts::HookFacts;

use super::banner::{Banner, Banners};
use super::new_session::{accounts, folders, NewSession};
use crate::accounts::menu::targets;
use crate::accounts::renders::{snapshot, Shown as AccountsShown};

pub const VIEWS: [ViewSpec; 7] = [
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
        size: (620.0, 1200.0),
        about: "the Settings sheet with acting shown read-only (defaults), and Notchling's five hooks (fixture reply)",
    },
    ViewSpec {
        name: "settings-hooks",
        size: (620.0, 1700.0),
        about: "the Settings sheet tall enough to show the Hooks section with Notchling's five hooks (fixture reply)",
    },
    ViewSpec {
        name: "settings-hooks-swift",
        size: (620.0, 1700.0),
        about: "the Settings sheet's Hooks section with a Swift app entry and an unmanaged one (fixture reply)",
    },
    ViewSpec {
        name: "settings-hooks-empty",
        size: (620.0, 1700.0),
        about: "the Settings sheet's Hooks section with none installed (fixture reply)",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Banner,
    NewSession,
    TrustFolder,
    Settings,
    SettingsHooks,
    SettingsHooksSwift,
    SettingsHooksEmpty,
}

pub fn shown(name: &str) -> Option<Shown> {
    Some(match name {
        "banner" => Shown::Banner,
        "new-session" => Shown::NewSession,
        "trust-folder" => Shown::TrustFolder,
        "settings" => Shown::Settings,
        "settings-hooks" => Shown::SettingsHooks,
        "settings-hooks-swift" => Shown::SettingsHooksSwift,
        "settings-hooks-empty" => Shown::SettingsHooksEmpty,
        _ => return None,
    })
}

pub fn acting(shown: Shown) -> Acting {
    match shown {
        Shown::TrustFolder => Acting::On,
        Shown::Banner
        | Shown::NewSession
        | Shown::Settings
        | Shown::SettingsHooks
        | Shown::SettingsHooksSwift
        | Shown::SettingsHooksEmpty => Acting::Off,
    }
}

fn hook(event: &str, matcher: &str, origin: &str, managed: bool) -> Value {
    json!({"event": event, "matcher": matcher, "origin": origin,
        "command": "'/Applications/Notchling.app/Contents/MacOS/notchling' hook",
        "managed": managed})
}

pub fn hooks(shown: Shown) -> HooksShown {
    let present = match shown {
        Shown::SettingsHooksEmpty => vec![],
        Shown::SettingsHooksSwift => vec![
            hook("PermissionRequest", "*", "swift", false),
            hook("PreToolUse", "AskUserQuestion", "notchling", true),
            hook("PostToolUse", "Artifact", "notchling", false),
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
        assert_eq!(hooks(Shown::Settings).lines().len(), 5);
        assert_eq!(hooks(Shown::SettingsHooksSwift).lines().len(), 3);
        assert_eq!(hooks(Shown::SettingsHooksEmpty), HooksShown::Empty);
    }
}
