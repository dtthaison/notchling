//! The daemon's three Milestone 6 switches as the handover checklist shows them.
//!
//! The state itself has one owner in the app, the switch feed (`crate::switches::feed`), which
//! follows the daemon's `settings.switch` events and is the only place a switch is turned on or off
//! (`SwitchFeed::choose`, after the checklist's confirmation). This is the checklist's view of it:
//! [`Known::from_feed`].

use crate::switches::{Switch, Switches};

pub const ACCOUNTS: &str = "accounts";
pub const VAULT: &str = "vault";
pub const INSTALL: &str = "install";
/// The switches, in the order they are shown.
pub const NAMES: [&str; 3] = [ACCOUNTS, VAULT, INSTALL];

/// What the daemon said about its switches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct States {
    pub accounts: bool,
    pub vault: bool,
    pub install: bool,
}

impl States {
    pub fn get(&self, name: &str) -> bool {
        match name {
            ACCOUNTS => self.accounts,
            VAULT => self.vault,
            INSTALL => self.install,
            _ => false,
        }
    }
}

/// What the app last knew.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Known {
    /// No daemon answered.
    Detached,
    States(States),
}

impl Known {
    /// The feed's switches as the checklist reads them.
    pub fn from_feed(s: Switches) -> Known {
        if !s.attached {
            return Known::Detached;
        }
        Known::States(States {
            accounts: s.is_on(Switch::Accounts),
            vault: s.is_on(Switch::Vault),
            install: s.is_on(Switch::Install),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feeds_switches_are_read_as_the_checklist_shows_them() {
        assert_eq!(Known::from_feed(Switches::detached()), Known::Detached);
        assert_eq!(
            Known::from_feed(Switches::all_off()),
            Known::States(States::default())
        );
        let mut s = Switches::all_off();
        s.set(Switch::Vault, true);
        let Known::States(states) = Known::from_feed(s) else {
            panic!("attached");
        };
        assert!(states.get(VAULT) && !states.get(ACCOUNTS) && !states.get(INSTALL));
        assert!(!states.get("acting"));
        for name in NAMES {
            assert!(Switch::parse(name).is_some(), "{name}");
        }
    }
}
