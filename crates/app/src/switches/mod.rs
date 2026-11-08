//! Milestone 6's switches as the app sees them: `accounts`, `vault` and `install`, each off at
//! every daemon start until an app-tier `settings.set_switch` turns it on (decision 0011).
//!
//! The app never guesses: a control that needs a switch is live only while the daemon is attached
//! and has said that switch is on ([`Switches::control`]); otherwise it is drawn disabled with the
//! daemon's own off message as its tooltip, word for word, so the user reads the same sentence a
//! refused op would give. The daemon's `settings.switch {switch, on}` events keep [`Switches`]
//! current ([`Switches::apply`]); the first read is `settings.switches`.
//!
//! The user's choice per switch is kept in the app's own `settings.json` (`accountsOn`, `vaultOn`,
//! `installOn`, like `actingOn`), but the app never turns a switch on from that file: a file any
//! agent can write must not hand out the vault or the logins. After a daemon restart the
//! [`restart`] notice offers the switches back, and only the person's confirmation sends them.

pub mod feed;
pub mod restart;
#[cfg(target_os = "macos")]
pub mod restart_view;

use notchling_local_api::{ClientError, ErrorCode};
use serde_json::{json, Value};

use crate::acting::{Control, DETACHED_MESSAGE};

/// What the daemon says, word for word, while each switch is off (`notchlingd`'s `switches`).
pub const ACCOUNTS_OFF: &str =
    "accounts are off: the Swift app owns logins until the maintainer turns this on";
pub const VAULT_OFF: &str =
    "the vault is off: the Swift app owns it until the maintainer turns this on";
pub const INSTALL_OFF: &str =
    "install is off: the Swift app owns hooks and skills until the maintainer turns this on";

/// One of the three switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Switch {
    Accounts,
    Vault,
    Install,
}

impl Switch {
    pub const ALL: [Switch; 3] = [Switch::Accounts, Switch::Vault, Switch::Install];

    /// The name on the wire.
    pub fn wire(self) -> &'static str {
        match self {
            Switch::Accounts => "accounts",
            Switch::Vault => "vault",
            Switch::Install => "install",
        }
    }

    pub fn parse(name: &str) -> Option<Switch> {
        Switch::ALL.into_iter().find(|s| s.wire() == name)
    }

    /// The daemon's refusal while this switch is off.
    pub fn off_message(self) -> &'static str {
        match self {
            Switch::Accounts => ACCOUNTS_OFF,
            Switch::Vault => VAULT_OFF,
            Switch::Install => INSTALL_OFF,
        }
    }

    /// The key in the app's `settings.json` that keeps the user's choice.
    pub fn choice_key(self) -> &'static str {
        match self {
            Switch::Accounts => "accountsOn",
            Switch::Vault => "vaultOn",
            Switch::Install => "installOn",
        }
    }

    /// Whether `message` is this switch's off refusal, however it is prefixed.
    pub fn is_off_message(self, message: &str) -> bool {
        message.contains(self.off_message())
    }

    /// The op that turns it on or off, and its arguments.
    pub fn set_request(self, on: bool) -> (&'static str, Value) {
        (
            notchling_local_api::ops::SETTINGS_SET_SWITCH,
            json!({"switch": self.wire(), "on": on}),
        )
    }
}

/// What the daemon last said about the switches, and whether it is there to say anything.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Switches {
    /// A daemon answered; without one every switch reads as detached.
    pub attached: bool,
    pub accounts: bool,
    pub vault: bool,
    pub install: bool,
}

impl Switches {
    /// Nothing known: a render's, a fixture root's.
    pub fn detached() -> Switches {
        Switches::default()
    }

    /// Every switch on, attached: a render that shows the live state.
    pub fn all_on() -> Switches {
        Switches {
            attached: true,
            accounts: true,
            vault: true,
            install: true,
        }
    }

    /// Attached, every switch off: what every daemon start looks like.
    pub fn all_off() -> Switches {
        Switches {
            attached: true,
            ..Switches::default()
        }
    }

    pub fn is_on(&self, switch: Switch) -> bool {
        self.attached
            && match switch {
                Switch::Accounts => self.accounts,
                Switch::Vault => self.vault,
                Switch::Install => self.install,
            }
    }

    pub fn set(&mut self, switch: Switch, on: bool) {
        match switch {
            Switch::Accounts => self.accounts = on,
            Switch::Vault => self.vault = on,
            Switch::Install => self.install = on,
        }
    }

    /// Why a control that needs `switch` is off, or `None` while it is live.
    pub fn message(&self, switch: Switch) -> Option<&'static str> {
        if !self.attached {
            Some(DETACHED_MESSAGE)
        } else if !self.is_on(switch) {
            Some(switch.off_message())
        } else {
            None
        }
    }

    /// How a control that needs `switch` is drawn.
    pub fn control(&self, switch: Switch) -> Control {
        let message = self.message(switch);
        Control {
            enabled: message.is_none(),
            tooltip: message,
        }
    }

    /// The Settings sheet's word for a switch.
    pub fn word(&self, switch: Switch) -> &'static str {
        if !self.attached {
            "Daemon not attached"
        } else if self.is_on(switch) {
            "On"
        } else {
            "Off"
        }
    }

    /// Reads `settings.switches`' reply (`{accounts, vault, install}`): a missing or odd field is
    /// off, since off is the daemon's default.
    pub fn from_reply(reply: &Value) -> Switches {
        let on = |key: &str| reply.get(key).and_then(Value::as_bool).unwrap_or(false);
        Switches {
            attached: true,
            accounts: on("accounts"),
            vault: on("vault"),
            install: on("install"),
        }
    }

    /// Applies a `settings.switch {switch, on}` event (or a `settings.set_switch` reply, the same
    /// shape). Whether anything changed.
    pub fn apply(&mut self, data: &Value) -> bool {
        let Some(switch) = data
            .get("switch")
            .and_then(Value::as_str)
            .and_then(Switch::parse)
        else {
            return false;
        };
        let on = data.get("on").and_then(Value::as_bool).unwrap_or(false);
        let before = *self;
        self.attached = true;
        self.set(switch, on);
        before != *self
    }
}

/// What a failed call says about a switch: the daemon's off refusal for it turns it off; a lost
/// connection detaches everything; any other error says nothing.
pub fn after_error(error: &ClientError, switch: Switch) -> Option<Switches> {
    match error {
        ClientError::Api(e)
            if e.code == ErrorCode::Forbidden && switch.is_off_message(&e.message) =>
        {
            Some(Switches {
                attached: true,
                ..Switches::default()
            })
        }
        _ => None,
    }
}

/// Whether this error means there is no daemon to talk to.
pub fn detaches(error: &ClientError) -> bool {
    crate::acting::after_error(error) == Some(crate::acting::Acting::Detached)
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_local_api::ApiError;

    #[test]
    fn the_names_round_trip_and_each_has_its_message() {
        for s in Switch::ALL {
            assert_eq!(Switch::parse(s.wire()), Some(s));
            assert!(s.is_off_message(&format!("forbidden: {}", s.off_message())));
            assert!(!s.is_off_message("acting is off"));
        }
        assert_eq!(Switch::parse("acting"), None);
        assert_eq!(Switch::Accounts.choice_key(), "accountsOn");
        let (op, args) = Switch::Vault.set_request(true);
        assert_eq!(op, "settings.set_switch");
        assert_eq!(args, json!({"switch": "vault", "on": true}));
    }

    #[test]
    fn detached_says_so_off_says_the_daemons_words_and_on_is_live() {
        let detached = Switches::detached();
        assert_eq!(
            detached.control(Switch::Accounts),
            Control {
                enabled: false,
                tooltip: Some(DETACHED_MESSAGE)
            }
        );
        assert_eq!(detached.word(Switch::Vault), "Daemon not attached");
        let off = Switches::all_off();
        assert_eq!(off.control(Switch::Vault).tooltip, Some(VAULT_OFF));
        assert_eq!(off.control(Switch::Install).tooltip, Some(INSTALL_OFF));
        assert_eq!(off.word(Switch::Accounts), "Off");
        let on = Switches::all_on();
        assert!(on.control(Switch::Accounts).enabled);
        assert_eq!(on.control(Switch::Accounts).tooltip, None);
        assert_eq!(on.word(Switch::Install), "On");
    }

    #[test]
    fn the_reply_and_the_events_are_read_with_off_as_the_default() {
        let s = Switches::from_reply(&json!({"accounts": true, "vault": "yes"}));
        assert!(s.attached && s.accounts && !s.vault && !s.install);
        let mut s = Switches::all_off();
        assert!(s.apply(&json!({"switch": "vault", "on": true})));
        assert!(s.vault);
        assert!(
            !s.apply(&json!({"switch": "vault", "on": true})),
            "no change"
        );
        assert!(s.apply(&json!({"switch": "vault"})), "a missing on is off");
        assert!(!s.vault);
        assert!(!s.apply(&json!({"switch": "acting", "on": true})));
        assert!(!s.apply(&json!("x")));
        // An event attaches a detached state.
        let mut d = Switches::detached();
        assert!(d.apply(&json!({"switch": "install", "on": true})));
        assert!(d.attached && d.is_on(Switch::Install));
    }

    #[test]
    fn the_off_refusal_turns_a_switch_off_and_a_lost_daemon_detaches() {
        let off = ClientError::Api(ApiError::new(ErrorCode::Forbidden, ACCOUNTS_OFF));
        let after = after_error(&off, Switch::Accounts).unwrap();
        assert!(after.attached && !after.accounts);
        assert_eq!(
            after_error(&off, Switch::Vault),
            None,
            "another switch's words"
        );
        let tier = ClientError::Api(ApiError::new(ErrorCode::Forbidden, "app tier only"));
        assert_eq!(after_error(&tier, Switch::Accounts), None);
        assert!(!detaches(&tier));
        assert!(detaches(&ClientError::Closed));
    }
}
