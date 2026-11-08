//! The views for `--render` (part U10): first run, About and the handover checklist, over made-up
//! data. Nothing is read from the real Mac and nothing is called.

use serde_json::json;

use super::about::{self, About};
use super::facts::{HookFacts, SwiftApp};
use super::first_run::FirstRun;
use super::handover::{confirm, Confirm, Control, State};
use super::switches::{Known, States};
use super::updates::Outcome;
use crate::acting::Acting;
use crate::phone::bridge::Snapshot;
use crate::phone::wire::BridgeState;
use crate::views::ViewSpec;

pub const VIEWS: [ViewSpec; 5] = [
    ViewSpec {
        name: "first-run",
        size: (640.0, 980.0),
        about: "the first-run view: the Swift app is running, no Swift hook, install shown as a dry run (made-up data)",
    },
    ViewSpec {
        name: "first-run-swift-hook",
        size: (640.0, 760.0),
        about: "the first-run view with the Swift app's --hook in settings.json: no install is offered (made-up data)",
    },
    ViewSpec {
        name: "about",
        size: (460.0, 560.0),
        about: "About: version, build, license, links, and the update line with releases off (made-up build)",
    },
    ViewSpec {
        name: "handover",
        size: (820.0, 1760.0),
        about: "the handover checklist: who owns each area, what turning it over means, the steps (made-up state)",
    },
    ViewSpec {
        name: "handover-confirm",
        size: (820.0, 1760.0),
        about: "the handover checklist with the confirmation for turning the phone bridge on (made-up state)",
    },
];

/// What a render shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    FirstRun,
    FirstRunSwiftHook,
    About,
    Handover,
    HandoverConfirm,
}

pub fn shown(name: &str) -> Option<Shown> {
    Some(match name {
        "first-run" => Shown::FirstRun,
        "first-run-swift-hook" => Shown::FirstRunSwiftHook,
        "about" => Shown::About,
        "handover" => Shown::Handover,
        "handover-confirm" => Shown::HandoverConfirm,
        _ => return None,
    })
}

const OFF: &str =
    "install is off: the Swift app owns hooks and skills until the maintainer turns this on";

fn plan(present: serde_json::Value, changes: serde_json::Value) -> HookFacts {
    let reply = json!({"on": false, "applied": false, "message": OFF,
        "plan": {"action": "install", "changed": true, "blocked": null,
            "hooks": {"file": "/Users/example/.claude/settings.json", "state": "present",
                "read_only": false, "refused": null, "changed": true, "changes": changes,
                "notes": ["your other hooks are kept; a backup of settings.json is made first"],
                "present": present},
            "skills": [{"name": "notchling-pages", "tool": "claude", "path": "/x",
                        "action": "write", "reason": null, "error": null}]}});
    HookFacts::parse(&reply).unwrap_or_default()
}

fn offer_plan() -> HookFacts {
    plan(
        json!([]),
        json!([
            "add PreToolUse (AskUserQuestion): '/Applications/Notchling.app/Contents/MacOS/notchling' hook (timeout 300 s)",
            "add PermissionRequest (*): '/Applications/Notchling.app/Contents/MacOS/notchling' hook (timeout 300 s)",
            "add PreToolUse (Artifact): '/Applications/Notchling.app/Contents/MacOS/notchling' hook (timeout 60 s)",
        ]),
    )
}

fn swift_plan() -> HookFacts {
    plan(
        json!([{"event": "PermissionRequest", "matcher": "*", "origin": "swift",
                "command": "'/Applications/Notchling.app/Contents/MacOS/Notchling' --hook",
                "managed": false}]),
        json!([]),
    )
}

/// The first-run model for a render.
pub fn first_run(shown: Shown) -> FirstRun {
    match shown {
        Shown::FirstRunSwiftHook => FirstRun::new(SwiftApp::Running, Ok(swift_plan())),
        _ => {
            let mut run = FirstRun::new(SwiftApp::Running, Ok(offer_plan()));
            // The button was pressed with install off: the plan and the off message come back.
            run.asked = Some(Ok(offer_plan()));
            run.imported = Some(Ok(super::defaults::Imported {
                added: (0..14).map(|n| format!("key{n}")).collect(),
                ..Default::default()
            }));
            run
        }
    }
}

/// A made-up build, so the picture doesn't change with the checkout.
pub fn about_model() -> About {
    about::about("0.1.0", Some("0bdc0b0"), true, "macos aarch64")
}

/// Releases are off in a render, so the line is the off message.
pub fn update_outcome() -> Outcome {
    Outcome::Off
}

/// The handover state for a render: acting on, accounts on, everything else off, the Swift hook in.
pub fn handover_state() -> State {
    State {
        swift: SwiftApp::Running,
        acting: Acting::On,
        bridge: Snapshot {
            attached: true,
            state: BridgeState::Off,
            fingerprint: None,
            pairing: None,
            devices: Vec::new(),
            tunnel: None,
            error: None,
        },
        switches: Known::States(States {
            accounts: true,
            vault: false,
            install: false,
        }),
        hook: Some(swift_plan()),
    }
}

/// The dialog shown by `handover-confirm`.
pub fn pending_confirm() -> Confirm {
    confirm(Control::Bridge, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::welcome::first_run::HookBlock;
    use crate::welcome::handover::{rows, Owner};

    #[test]
    fn every_view_has_its_data() {
        for spec in VIEWS {
            assert!(shown(spec.name).is_some(), "{}", spec.name);
            assert!(spec.size.0 > 0.0 && spec.size.1 > 0.0);
        }
        assert_eq!(shown("chat"), None);
    }

    #[test]
    fn the_first_run_renders_show_the_two_hook_cases() {
        assert!(matches!(
            first_run(Shown::FirstRun).hook_block(),
            HookBlock::Asked { applied: false, .. }
        ));
        assert_eq!(
            first_run(Shown::FirstRunSwiftHook).hook_block(),
            HookBlock::SwiftOwns { entries: 1 }
        );
    }

    #[test]
    fn the_handover_render_mixes_owners() {
        let rows = rows(&handover_state());
        let owners: Vec<Owner> = rows.iter().map(|r| r.owner).collect();
        assert!(owners.contains(&Owner::Notchling) && owners.contains(&Owner::Swift));
        assert!(pending_confirm().turn_on);
        assert_eq!(update_outcome(), Outcome::Off);
        assert_eq!(about_model().version, "0.1.0");
    }
}
