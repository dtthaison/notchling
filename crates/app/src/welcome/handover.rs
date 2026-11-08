//! The handover checklist's model (decision 0011): one row per resource that has one owner at a
//! time, who owns it now, what turning it over means and the exact steps for the maintainer, and
//! the switch that does it (`acting`, `bridge`, `accounts`, `vault`, `install`). Pure; the view
//! draws it, and every toggle asks for a [`Confirm`] first.

use notchling_local_api::ops;

use super::facts::{HookFacts, SwiftApp};
use super::switches::{Known, ACCOUNTS, INSTALL, VAULT};
use crate::acting::{Acting, DETACHED_MESSAGE};
use crate::phone::bridge::Snapshot;
use crate::phone::wire::BridgeState;

pub const TITLE: &str = "Handover";
pub const INTRO: &str = "Each area has one owner at a time. The Swift app owns all of them until you turn one over here. Nothing is turned over by itself, and every switch is off again whenever the daemon starts. This app never turns one back on by itself: it shows a notice, and only your Turn Back On does it.";

/// Who has a resource now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Swift,
    /// This app's daemon.
    Notchling,
    /// Neither: nothing is installed.
    Nobody,
    /// The daemon isn't attached, so nothing is known.
    Unknown,
}

impl Owner {
    pub fn word(self) -> &'static str {
        match self {
            Owner::Swift => "Swift app",
            Owner::Notchling => "This app",
            Owner::Nobody => "Nobody",
            Owner::Unknown => "Unknown",
        }
    }
}

/// What a toggle changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// `settings.set_acting`.
    Acting,
    /// `bridge.set`.
    Bridge,
    /// `settings.set_switch` for `accounts`, `vault` or `install`.
    Switch(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toggle {
    pub control: Control,
    pub on: bool,
    pub enabled: bool,
    /// Why it can't be used now.
    pub why: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: &'static str,
    pub title: &'static str,
    pub owner: Owner,
    /// One line about the state, beside the owner.
    pub status: String,
    /// What turning it over means.
    pub meaning: &'static str,
    /// The maintainer's steps, in order.
    pub steps: &'static [&'static str],
    pub toggle: Option<Toggle>,
}

/// Everything the checklist is drawn from.
#[derive(Clone, Debug)]
pub struct State {
    pub swift: SwiftApp,
    pub acting: Acting,
    pub bridge: Snapshot,
    pub switches: Known,
    pub hook: Option<HookFacts>,
}

const ACTING_STEPS: &[&str] = &[
    "Record the reply fixtures against a real session (docs/recording.md).",
    "Turn this on. The daemon starts with it off every time; after a restart this app offers it back in a notice and turns it on only when you confirm.",
    "Try a reply and an answer from Chat. The Swift app keeps the hook until the hook row below is handed over.",
];

const HOOK_STEPS: &[&str] = &[
    "Retire the Swift app's hook entries in ~/.claude/settings.json (decision 0011: retirement rewrites the command and leaves a backup). While one is there, install is refused.",
    "Turn this on. It only allows the install ops to write.",
    "Use Install Hook in Welcome (Notchling menu). A backup of settings.json is made first and your other hooks are kept.",
];

const HOSTD_STEPS: &[&str] = &[
    "Nothing to do yet. The Swift app keeps the hostd LaunchAgent (local.agentconsole.hostd) and hostd.sock until it is retired.",
    "At retirement the ported terminal host takes the same socket.",
];

const BRIDGE_STEPS: &[&str] = &[
    "Stop the Swift bridge first: turn off Let phones connect in the Swift app's Phone panel, or quit the Swift app. Both write AgentConsole/bridge/devices.json without a lock, and the ports clash.",
    "Turn this on. The daemon carries cert.pem and key.pem over byte for byte, so phones stay paired.",
    "Open the Phone panel here and check the phones are listed.",
];

const VAULT_STEPS: &[&str] = &[
    "Quit the Swift app: it holds vault.sock and the vault Keychain item.",
    "Turn this on. macOS asks once for access to the vault item; choose Always Allow.",
    "Check the vault sheet lists your secrets. The daemon keeps its own list in its daemon folder and never writes the Swift app's vault.json.",
];

const ACCOUNTS_STEPS: &[&str] = &[
    "In the Swift app, set the Claude Code, Codex and Antigravity limit modes to Off (Settings, Limits), so limits are never watched twice.",
    "Quit the Swift app, or leave its Accounts window closed, so only one side switches a login.",
    "Turn this on. Lane Keychain names stay as they are, and no token is renewed while it is in use.",
];

const LIMITS_STEPS: &[&str] = &[
    "Follows the Logins and lanes switch above; there is no switch of its own.",
    "Never both: Swift's watcher must be off first (step 1 above).",
];

/// Reads one switch from what the daemon said.
fn switch_on(known: &Known, name: &str) -> Option<bool> {
    match known {
        Known::States(s) => Some(s.get(name)),
        Known::Detached => None,
    }
}

fn owner_of(on: Option<bool>) -> Owner {
    match on {
        Some(true) => Owner::Notchling,
        Some(false) => Owner::Swift,
        None => Owner::Unknown,
    }
}

fn toggle_for(control: Control, on: Option<bool>) -> Toggle {
    Toggle {
        control,
        on: on.unwrap_or(false),
        enabled: on.is_some(),
        why: on.is_none().then_some(DETACHED_MESSAGE),
    }
}

fn status(owner: Owner, swift: SwiftApp, on_line: &str, off_line: &str) -> String {
    match owner {
        Owner::Notchling => on_line.to_owned(),
        Owner::Swift if swift == SwiftApp::NotFound => {
            format!("{off_line} (No Swift app was found on this Mac.)")
        }
        Owner::Swift => off_line.to_owned(),
        Owner::Nobody => "Nothing is installed.".to_owned(),
        Owner::Unknown => "The daemon isn't attached, so this isn't known.".to_owned(),
    }
}

/// The checklist, top to bottom.
pub fn rows(state: &State) -> Vec<Row> {
    let acting = match state.acting {
        Acting::On => Some(true),
        Acting::Off => Some(false),
        Acting::Detached => None,
    };
    let acting_owner = owner_of(acting);
    let bridge_known = state.bridge.attached;
    let bridge_on = bridge_known.then(|| state.bridge.state.is_on());
    let bridge_owner = owner_of(bridge_on);
    let bridge_line = match (&state.bridge.state, bridge_known) {
        (BridgeState::Failed(why), true) => format!("The daemon's bridge failed to start: {why}"),
        (BridgeState::Starting, true) => "The daemon's bridge is starting.".to_owned(),
        _ => status(
            bridge_owner,
            state.swift,
            "The daemon's bridge is listening.",
            "The daemon's bridge is off.",
        ),
    };
    let accounts = switch_on(&state.switches, ACCOUNTS);
    let vault = switch_on(&state.switches, VAULT);
    let install = switch_on(&state.switches, INSTALL);
    let hook_owner = match &state.hook {
        Some(h) if h.swift_owns_hook() => Owner::Swift,
        Some(h) if h.we_own_hook() => Owner::Notchling,
        Some(_) => Owner::Nobody,
        None => Owner::Unknown,
    };
    let hook_status = match (&state.hook, hook_owner) {
        (Some(h), Owner::Swift) => format!(
            "Claude Code's settings.json has the Swift app's --hook ({}).",
            h.swift_entries.len()
        ),
        (Some(_), Owner::Notchling) => "settings.json has this app's hook.".to_owned(),
        (_, owner) => status(owner, state.swift, "", ""),
    };
    let accounts_owner = owner_of(accounts);
    vec![
        Row {
            id: "sessions",
            title: "Sessions",
            owner: acting_owner,
            status: status(
                acting_owner,
                state.swift,
                "Replies, answers, stop and new sessions go through the daemon.",
                "Acting is off: the daemon sends nothing to a session.",
            ),
            meaning: "Replies, answers, permission choices, stop, take-back and new sessions are sent by this app's daemon instead of the Swift app.",
            steps: ACTING_STEPS,
            toggle: Some(toggle_for(Control::Acting, acting)),
        },
        Row {
            id: "hook",
            title: "Hook entries",
            owner: hook_owner,
            status: hook_status,
            meaning: "The daemon's install ops may write notchling hook entries into ~/.claude/settings.json and the notchling skills into Claude Code's, Codex's and Antigravity's folders.",
            steps: HOOK_STEPS,
            toggle: Some(toggle_for(Control::Switch(INSTALL), install)),
        },
        Row {
            id: "hostd",
            title: "Terminal host",
            owner: Owner::Swift,
            status: "Kept by the Swift app until it is retired.".to_owned(),
            meaning: "The pty host stays the Swift app's LaunchAgent. There is no switch for it.",
            steps: HOSTD_STEPS,
            toggle: None,
        },
        Row {
            id: "bridge",
            title: "Phone bridge",
            owner: bridge_owner,
            status: bridge_line,
            meaning: "The daemon listens on ports 47821 and 47822 and keeps the paired phones, certificate and devices file.",
            steps: BRIDGE_STEPS,
            toggle: Some(toggle_for(Control::Bridge, bridge_on)),
        },
        Row {
            id: "vault",
            title: "Secret Vault",
            owner: owner_of(vault),
            status: status(
                owner_of(vault),
                state.swift,
                "The daemon opens the vault Keychain item and binds vault.sock.",
                "The daemon binds no vault socket and opens no Keychain item.",
            ),
            meaning: "The daemon holds vault.sock and the vault Keychain item, answers secret requests and shows the approval prompt through this app.",
            steps: VAULT_STEPS,
            toggle: Some(toggle_for(Control::Switch(VAULT), vault)),
        },
        Row {
            id: "accounts",
            title: "Logins and lanes",
            owner: accounts_owner,
            status: status(
                accounts_owner,
                state.swift,
                "The daemon reads and switches logins and lanes.",
                "The daemon reads no login and switches nothing.",
            ),
            meaning: "The daemon reads the Keychain logins, switches accounts and lanes, and changes only claudeAiOauth and oauthAccount.",
            steps: ACCOUNTS_STEPS,
            toggle: Some(toggle_for(Control::Switch(ACCOUNTS), accounts)),
        },
        Row {
            id: "limits",
            title: "Limit watching",
            owner: accounts_owner,
            status: status(
                accounts_owner,
                state.swift,
                "The daemon polls limits and may switch at the set percent.",
                "The daemon polls no limits and switches nothing.",
            ),
            meaning: "Limit polling and auto-switch run in the daemon, never in both apps at once.",
            steps: LIMITS_STEPS,
            toggle: None,
        },
    ]
}

/// A dialog that names what a toggle changes, shown before anything is sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirm {
    pub control: Control,
    pub turn_on: bool,
    pub title: String,
    pub lines: Vec<&'static str>,
    pub ok: &'static str,
}

const KEEPS_CHOICE: &str =
    "This app keeps your choice and asks the daemon for it again at every start.";

/// The dialog for turning `control` on or off.
pub fn confirm(control: Control, turn_on: bool) -> Confirm {
    let (name, on_lines, off_lines): (&str, &[&'static str], &[&'static str]) = match control {
        Control::Acting => (
            "sessions",
            &[
                "The daemon will send replies, answers, permission choices, stop and new sessions to your Claude Code sessions.",
                "If the Swift app's hook is still installed, both can answer the same prompt.",
            ],
            &["The daemon will send nothing to any session. The Swift app is unchanged."],
        ),
        Control::Bridge => (
            "the phone bridge",
            &[
                "The daemon will listen on ports 47821 and 47822 and take over the paired phones.",
                "Stop the Swift bridge first: the ports clash, and both write devices.json without a lock.",
            ],
            &["The daemon will stop listening. Phones can't connect until a bridge is on again."],
        ),
        Control::Switch(ACCOUNTS) => (
            "logins, lanes and limit watching",
            &[
                "The daemon will read the Keychain logins, switch accounts and lanes, and may switch on its own at the limit.",
                "Turn off the Swift app's limit watching first: two watchers must never run.",
            ],
            &["The daemon will read no login, switch nothing and stop watching limits."],
        ),
        Control::Switch(VAULT) => (
            "the Secret Vault",
            &[
                "The daemon will open the vault Keychain item (macOS asks once) and bind vault.sock.",
                "Quit the Swift app first: it holds the socket.",
            ],
            &["The daemon will release vault.sock and answer every vault request with the off message."],
        ),
        Control::Switch(_) => (
            "hook and skill install",
            &[
                "The install ops may write notchling hook entries into ~/.claude/settings.json (with a backup) and the notchling skills into the tools' skills folders.",
                "While the Swift app's --hook is in settings.json, an install is refused.",
            ],
            &["The install ops go back to showing what they would do, and write nothing."],
        ),
    };
    let verb = if turn_on { "Turn on" } else { "Turn off" };
    let mut lines: Vec<&'static str> = if turn_on { on_lines } else { off_lines }.to_vec();
    lines.push(KEEPS_CHOICE);
    Confirm {
        control,
        turn_on,
        title: format!("{verb} {name}?"),
        lines,
        ok: if turn_on { "Turn On" } else { "Turn Off" },
    }
}

/// The op a control calls, for the audit-minded: shown in tests, not on screen.
pub fn op_of(control: Control) -> &'static str {
    match control {
        Control::Acting => ops::SETTINGS_SET_ACTING,
        Control::Bridge => ops::BRIDGE_SET,
        Control::Switch(_) => ops::SETTINGS_SET_SWITCH,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::welcome::switches::States;
    use serde_json::json;

    fn state() -> State {
        State {
            swift: SwiftApp::Running,
            acting: Acting::Off,
            bridge: Snapshot {
                attached: true,
                state: BridgeState::Off,
                fingerprint: None,
                pairing: None,
                devices: vec![],
                tunnel: None,
                error: None,
            },
            switches: Known::States(States::default()),
            hook: None,
        }
    }

    fn row<'a>(rows: &'a [Row], id: &str) -> &'a Row {
        rows.iter().find(|r| r.id == id).unwrap()
    }

    fn swift_hook() -> HookFacts {
        HookFacts::parse(
            &json!({"on": false, "applied": false, "plan": {"blocked": null,
            "skills": [], "hooks": {"state": "present", "refused": null, "changes": [], "notes": [],
            "present": [{"event": "PermissionRequest", "matcher": "*", "origin": "swift",
                         "command": "x --hook", "managed": false}]}}}),
        )
        .unwrap()
    }

    #[test]
    fn every_resource_and_switch_has_a_row() {
        let rows = rows(&state());
        let ids: Vec<&str> = rows.iter().map(|r| r.id).collect();
        assert_eq!(
            ids,
            ["sessions", "hook", "hostd", "bridge", "vault", "accounts", "limits"]
        );
        // Each switch has exactly one toggle: acting, bridge, accounts, vault, install.
        let controls: Vec<Control> = rows
            .iter()
            .filter_map(|r| r.toggle.as_ref().map(|t| t.control))
            .collect();
        assert_eq!(
            controls,
            [
                Control::Acting,
                Control::Switch("install"),
                Control::Bridge,
                Control::Switch("vault"),
                Control::Switch("accounts"),
            ]
        );
        for r in &rows {
            assert!(!r.steps.is_empty() && !r.meaning.is_empty(), "{}", r.id);
        }
    }

    #[test]
    fn all_off_means_the_swift_app_owns_everything() {
        let mut s = state();
        s.hook = Some(swift_hook());
        let rows = rows(&s);
        for r in &rows {
            assert_eq!(r.owner, Owner::Swift, "{}", r.id);
        }
        assert!(row(&rows, "hook").status.contains("--hook"));
        assert!(row(&rows, "hostd").toggle.is_none());
        assert!(row(&rows, "limits").toggle.is_none());
    }

    #[test]
    fn a_switch_on_moves_its_row_and_the_limits_follow_accounts() {
        let mut s = state();
        s.acting = Acting::On;
        s.switches = Known::States(States {
            accounts: true,
            vault: false,
            install: false,
        });
        s.bridge.state = BridgeState::On(Some(47821));
        let rows = rows(&s);
        assert_eq!(row(&rows, "sessions").owner, Owner::Notchling);
        assert_eq!(row(&rows, "accounts").owner, Owner::Notchling);
        assert_eq!(row(&rows, "limits").owner, Owner::Notchling);
        assert_eq!(row(&rows, "bridge").owner, Owner::Notchling);
        assert_eq!(row(&rows, "vault").owner, Owner::Swift);
        let t = row(&rows, "accounts").toggle.clone().unwrap();
        assert!(t.on && t.enabled && t.why.is_none());
    }

    #[test]
    fn a_detached_daemon_disables_every_toggle_with_the_reason() {
        let mut s = state();
        s.acting = Acting::Detached;
        s.bridge = Snapshot::detached();
        s.switches = Known::Detached;
        let rows = rows(&s);
        for r in rows.iter().filter(|r| r.toggle.is_some()) {
            assert_eq!(r.owner, Owner::Unknown, "{}", r.id);
            let t = r.toggle.as_ref().unwrap();
            assert!(!t.enabled && !t.on, "{}", r.id);
            assert_eq!(t.why, Some(DETACHED_MESSAGE));
        }
    }

    #[test]
    fn the_hook_row_tells_who_has_it() {
        let mut s = state();
        assert_eq!(row(&rows(&s), "hook").owner, Owner::Unknown);
        s.hook = Some(
            HookFacts::parse(&json!({"on": false, "applied": false, "plan": {
            "blocked": null, "skills": [], "hooks": {"state": "empty", "refused": null,
            "changes": [], "notes": [], "present": []}}}))
            .unwrap(),
        );
        assert_eq!(row(&rows(&s), "hook").owner, Owner::Nobody);
        s.hook = Some(swift_hook());
        assert_eq!(row(&rows(&s), "hook").owner, Owner::Swift);
    }

    #[test]
    fn a_failed_bridge_says_why_and_stays_swifts() {
        let mut s = state();
        s.bridge.state = BridgeState::Failed("port 47821 is in use".into());
        let rows = rows(&s);
        let bridge = row(&rows, "bridge");
        assert!(bridge.status.contains("port 47821 is in use"));
        assert_eq!(bridge.owner, Owner::Swift);
    }

    #[test]
    fn the_steps_say_what_the_decision_notes_say() {
        let rows = rows(&state());
        let bridge = row(&rows, "bridge").steps.join(" ");
        assert!(bridge.contains("Stop the Swift bridge first") && bridge.contains("devices.json"));
        assert!(row(&rows, "sessions")
            .steps
            .join(" ")
            .contains("reply fixtures"));
        assert!(row(&rows, "vault").steps.join(" ").contains("Always Allow"));
        assert!(row(&rows, "accounts")
            .steps
            .join(" ")
            .contains("limit modes"));
        assert!(row(&rows, "hook")
            .steps
            .join(" ")
            .contains("install is refused"));
    }

    #[test]
    fn every_confirmation_names_the_change_and_says_the_choice_is_kept() {
        let controls = [
            Control::Acting,
            Control::Bridge,
            Control::Switch("accounts"),
            Control::Switch("vault"),
            Control::Switch("install"),
        ];
        for control in controls {
            for turn_on in [true, false] {
                let c = confirm(control, turn_on);
                assert!(c
                    .title
                    .starts_with(if turn_on { "Turn on" } else { "Turn off" }));
                assert!(c.title.ends_with('?'));
                assert!(c.lines.len() >= 2);
                assert_eq!(c.lines.last().copied(), Some(KEEPS_CHOICE));
                assert_eq!(c.ok, if turn_on { "Turn On" } else { "Turn Off" });
            }
        }
        let bridge = confirm(Control::Bridge, true);
        assert!(bridge
            .lines
            .iter()
            .any(|l| l.contains("Stop the Swift bridge first")));
        assert!(confirm(Control::Switch("vault"), true)
            .lines
            .iter()
            .any(|l| l.contains("Quit the Swift app first")));
    }

    #[test]
    fn each_control_goes_through_its_own_op() {
        assert_eq!(op_of(Control::Acting), "settings.set_acting");
        assert_eq!(op_of(Control::Bridge), "bridge.set");
        assert_eq!(op_of(Control::Switch("vault")), "settings.set_switch");
    }
}
