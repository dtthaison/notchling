pub mod feed;
pub mod restart;
#[cfg(target_os = "macos")]
pub mod restart_view;

use notchling_local_api::ClientError;
use serde_json::{json, Value};

use crate::acting::{Control, DETACHED_MESSAGE};

pub const ACCOUNTS_OFF: &str =
    "accounts are off: the Swift app owns logins until the maintainer turns this on";
pub const VAULT_OFF: &str =
    "the vault is off: the Swift app owns it until the maintainer turns this on";
pub const INSTALL_OFF: &str =
    "install is off: the Swift app owns hooks and skills until the maintainer turns this on";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Switch {
    Accounts,
    Vault,
    Install,
}

impl Switch {
    pub const ALL: [Switch; 3] = [Switch::Accounts, Switch::Vault, Switch::Install];

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

    pub fn off_message(self) -> &'static str {
        match self {
            Switch::Accounts => ACCOUNTS_OFF,
            Switch::Vault => VAULT_OFF,
            Switch::Install => INSTALL_OFF,
        }
    }

    pub fn choice_key(self) -> &'static str {
        match self {
            Switch::Accounts => "accountsOn",
            Switch::Vault => "vaultOn",
            Switch::Install => "installOn",
        }
    }

    pub fn set_request(self, on: bool) -> (&'static str, Value) {
        (
            notchling_local_api::ops::SETTINGS_SET_SWITCH,
            json!({"switch": self.wire(), "on": on}),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Switches {
    pub attached: bool,
    pub accounts: bool,
    pub vault: bool,
    pub install: bool,
}

impl Switches {
    pub fn detached() -> Switches {
        Switches::default()
    }

    pub fn all_on() -> Switches {
        Switches {
            attached: true,
            accounts: true,
            vault: true,
            install: true,
        }
    }

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

    pub fn message(&self, switch: Switch) -> Option<&'static str> {
        if !self.attached {
            Some(DETACHED_MESSAGE)
        } else if !self.is_on(switch) {
            Some(switch.off_message())
        } else {
            None
        }
    }

    pub fn control(&self, switch: Switch) -> Control {
        let message = self.message(switch);
        Control {
            enabled: message.is_none(),
            tooltip: message,
        }
    }

    pub fn word(&self, switch: Switch) -> &'static str {
        if !self.attached {
            "Daemon not attached"
        } else if self.is_on(switch) {
            "On"
        } else {
            "Off"
        }
    }

    pub fn from_reply(reply: &Value) -> Switches {
        let on = |key: &str| reply.get(key).and_then(Value::as_bool).unwrap_or(false);
        Switches {
            attached: true,
            accounts: on("accounts"),
            vault: on("vault"),
            install: on("install"),
        }
    }

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

pub fn detaches(error: &ClientError) -> bool {
    crate::acting::after_error(error) == Some(crate::acting::Acting::Detached)
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_local_api::{ApiError, ErrorCode};

    #[test]
    fn the_names_round_trip_and_each_has_its_message() {
        for s in Switch::ALL {
            assert_eq!(Switch::parse(s.wire()), Some(s));
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
        let mut d = Switches::detached();
        assert!(d.apply(&json!({"switch": "install", "on": true})));
        assert!(d.attached && d.is_on(Switch::Install));
    }

    #[test]
    fn only_a_lost_daemon_detaches() {
        let off = ClientError::Api(ApiError::new(ErrorCode::Forbidden, ACCOUNTS_OFF));
        assert!(!detaches(&off));
        let tier = ClientError::Api(ApiError::new(ErrorCode::Forbidden, "app tier only"));
        assert!(!detaches(&tier));
        assert!(detaches(&ClientError::Closed));
    }
}
