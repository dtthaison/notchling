//! What the app offers after the daemon restarted with switches the user had on.
//!
//! Every switch (`acting`, the phone bridge, `accounts`, `vault`, `install`) is off at every daemon
//! start, and the app never turns one back on by itself: a file any agent can write must not be
//! able to hand out the vault or the logins. Instead, when a switch this app saw on (or the
//! remembered choice in the app's `settings.json` says on) reads off, [`Restart`] offers it: one
//! notice, "Notchling restarted: Sessions, Vault are off", with Turn Back On, which opens one
//! confirmation naming each switch. Only that confirmation sends anything ([`turn_back_on`]).
//!
//! The remembered choice only decides whether the notice is offered; it never acts.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use super::feed::SwitchFeed;
use super::Switch;
use crate::actions::Actions;
use crate::phone::bridge::Bridge;

/// One switch the notice can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Area {
    Acting,
    Bridge,
    Accounts,
    Vault,
    Install,
}

impl Area {
    pub const ALL: [Area; 5] = [
        Area::Acting,
        Area::Bridge,
        Area::Accounts,
        Area::Vault,
        Area::Install,
    ];

    /// The word the notice uses.
    pub fn title(self) -> &'static str {
        match self {
            Area::Acting => "Sessions",
            Area::Bridge => "Phone Bridge",
            Area::Accounts => "Logins",
            Area::Vault => "Vault",
            Area::Install => "Install",
        }
    }

    /// What turning it back on lets this app do, for the confirmation.
    pub fn meaning(self) -> &'static str {
        match self {
            Area::Acting => "Sessions: this app replies, answers and acts on live sessions.",
            Area::Bridge => "Phone Bridge: paired phones can connect to this Mac.",
            Area::Accounts => {
                "Logins: this app switches accounts, manages lanes and watches limits."
            }
            Area::Vault => "Vault: agents can ask to use your saved secrets.",
            Area::Install => "Install: this app may write hooks and skills.",
        }
    }

    pub fn of(switch: Switch) -> Area {
        match switch {
            Switch::Accounts => Area::Accounts,
            Switch::Vault => Area::Vault,
            Switch::Install => Area::Install,
        }
    }
}

/// What the daemon says about a switch right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Said {
    /// No daemon to ask: nothing is learnt.
    Detached,
    On,
    Off,
}

impl Said {
    pub fn from_bool(on: bool) -> Said {
        if on {
            Said::On
        } else {
            Said::Off
        }
    }
}

/// The confirmation Turn Back On opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirm {
    pub title: &'static str,
    pub lines: Vec<String>,
    pub ok: &'static str,
    pub areas: Vec<Area>,
}

pub const NOTICE_BUTTON: &str = "Turn Back On";

/// `--render` names for the notice (`restart_view`).
pub const VIEWS: [crate::views::ViewSpec; 2] = [
    crate::views::ViewSpec {
        name: "restart-notice",
        size: super::restart_view::NOTICE_SIZE,
        about:
            "the notice after a daemon restart: Sessions, Logins and Vault were on (made-up state)",
    },
    crate::views::ViewSpec {
        name: "restart-notice-confirm",
        size: super::restart_view::CONFIRM_SIZE,
        about: "the restart notice's one confirmation, naming each switch (made-up state)",
    },
];
pub const CONFIRM_TITLE: &str = "Turn These Back On?";

/// The notice's state. Pure.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Restart {
    /// Switches this app process saw on, or the user turned on here.
    seen_on: BTreeSet<Area>,
    /// Switches on offer.
    offered: BTreeSet<Area>,
    /// Switches the user turned off, or whose offer was dismissed: not offered again until seen
    /// on again.
    quiet: BTreeSet<Area>,
}

impl Restart {
    /// What the daemon says now, and whether the remembered choice says on.
    pub fn observe(&mut self, area: Area, said: Said, remembered: bool) {
        match said {
            Said::Detached => {}
            Said::On => {
                self.seen_on.insert(area);
                self.offered.remove(&area);
                self.quiet.remove(&area);
            }
            Said::Off => {
                if (self.seen_on.contains(&area) || remembered) && !self.quiet.contains(&area) {
                    self.offered.insert(area);
                }
            }
        }
    }

    /// The user turned a switch on or off in this app (or the daemon said it was turned off while
    /// the app followed it): no notice for it.
    pub fn chose(&mut self, area: Area, on: bool) {
        self.offered.remove(&area);
        if on {
            self.seen_on.insert(area);
            self.quiet.remove(&area);
        } else {
            self.seen_on.remove(&area);
            self.quiet.insert(area);
        }
    }

    /// Not now: the notice goes until one of them is seen on again.
    pub fn dismiss(&mut self) {
        let offered = std::mem::take(&mut self.offered);
        self.quiet.extend(offered);
    }

    pub fn offer(&self) -> Vec<Area> {
        self.offered.iter().copied().collect()
    }

    /// "Notchling restarted: Sessions, Vault are off", or nothing to say.
    pub fn banner(&self) -> Option<String> {
        let names: Vec<&str> = self.offered.iter().map(|a| a.title()).collect();
        match names.len() {
            0 => None,
            1 => Some(format!("Notchling restarted: {} is off", names[0])),
            _ => Some(format!("Notchling restarted: {} are off", names.join(", "))),
        }
    }

    /// The one confirmation, naming each switch it turns on.
    pub fn confirm(&self) -> Option<Confirm> {
        if self.offered.is_empty() {
            return None;
        }
        let mut lines: Vec<String> = self
            .offered
            .iter()
            .map(|a| a.meaning().to_owned())
            .collect();
        lines.push(
            "Each one was off because the daemon started again. Nothing is turned on until you confirm here."
                .into(),
        );
        Some(Confirm {
            title: CONFIRM_TITLE,
            lines,
            ok: NOTICE_BUTTON,
            areas: self.offer(),
        })
    }
}

/// The notice, shared by the parts that learn what the daemon says and the window that shows it.
#[derive(Default)]
pub struct RestartNotice(Mutex<Restart>);

impl RestartNotice {
    fn with<T>(&self, f: impl FnOnce(&mut Restart) -> T) -> T {
        f(&mut self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn observe(&self, area: Area, said: Said, remembered: bool) {
        self.with(|r| r.observe(area, said, remembered));
    }

    pub fn chose(&self, area: Area, on: bool) {
        self.with(|r| r.chose(area, on));
    }

    pub fn dismiss(&self) {
        self.with(Restart::dismiss);
    }

    pub fn state(&self) -> Restart {
        self.with(|r| r.clone())
    }
}

/// What turns a switch on, for [`turn_back_on`]: the app's own sources in the app, a fake in tests.
pub trait TurnOn: Send + Sync {
    fn turn_on(&self, area: Area) -> Result<(), String>;
}

/// The running app's: each area through the one place that turns it on.
pub struct LiveTurnOn {
    pub actions: Arc<Actions>,
    pub bridge: Arc<Bridge>,
    pub feed: Arc<SwitchFeed>,
}

impl LiveTurnOn {
    /// Turns `area` on or off (Settings' switches). Blocking.
    pub fn set(&self, area: Area, on: bool) -> Result<(), String> {
        let switch = match area {
            Area::Acting => return self.actions.choose(on).map(|_| ()),
            Area::Bridge => return self.bridge.choose(on),
            Area::Accounts => Switch::Accounts,
            Area::Vault => Switch::Vault,
            Area::Install => Switch::Install,
        };
        self.feed.choose(switch, on).map(|_| ())
    }
}

impl TurnOn for LiveTurnOn {
    fn turn_on(&self, area: Area) -> Result<(), String> {
        self.set(area, true)
    }
}

/// The user confirmed: turns on exactly `areas`, each through its own op. The failures, each
/// with its area's word.
pub fn turn_back_on(areas: &[Area], with: &dyn TurnOn) -> Vec<String> {
    areas
        .iter()
        .filter_map(|a| {
            with.turn_on(*a)
                .err()
                .map(|e| format!("{}: {e}", a.title()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Recorder(Mutex<Vec<Area>>);

    impl TurnOn for Recorder {
        fn turn_on(&self, area: Area) -> Result<(), String> {
            self.0.lock().unwrap().push(area);
            if area == Area::Install {
                Err("app tier only".into())
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn a_remembered_choice_only_offers_and_the_confirm_turns_on_exactly_those() {
        let mut r = Restart::default();
        for area in Area::ALL {
            // The file says on for everything; the daemon says off for everything.
            r.observe(area, Said::Off, true);
        }
        assert_eq!(r.offer(), Area::ALL.to_vec());
        assert_eq!(
            r.banner().as_deref(),
            Some("Notchling restarted: Sessions, Phone Bridge, Logins, Vault, Install are off")
        );
        let confirm = r.confirm().unwrap();
        assert_eq!(confirm.ok, NOTICE_BUTTON);
        assert_eq!(confirm.areas, Area::ALL.to_vec());
        assert!(confirm.lines[3].starts_with("Vault"), "{:?}", confirm.lines);
        let recorder = Recorder(Mutex::new(Vec::new()));
        let failed = turn_back_on(&confirm.areas, &recorder);
        assert_eq!(*recorder.0.lock().unwrap(), Area::ALL.to_vec());
        assert_eq!(failed, ["Install: app tier only"]);
    }

    #[test]
    fn on_in_memory_then_off_is_a_restart_and_nothing_known_is_not() {
        let mut r = Restart::default();
        r.observe(Area::Vault, Said::On, false);
        r.observe(Area::Acting, Said::Off, false);
        r.observe(Area::Bridge, Said::Detached, true);
        assert_eq!(r.banner(), None, "never on, never chosen: nothing to offer");
        // The daemon restarts: off again.
        r.observe(Area::Vault, Said::Detached, false);
        assert_eq!(r.offer(), []);
        r.observe(Area::Vault, Said::Off, false);
        assert_eq!(r.offer(), [Area::Vault]);
        assert_eq!(
            r.banner().as_deref(),
            Some("Notchling restarted: Vault is off")
        );
        // Turned back on elsewhere: the offer goes.
        r.observe(Area::Vault, Said::On, false);
        assert_eq!(r.offer(), []);
    }

    #[test]
    fn the_users_off_and_a_dismissal_quiet_it_until_seen_on_again() {
        let mut r = Restart::default();
        r.observe(Area::Accounts, Said::On, true);
        r.chose(Area::Accounts, false);
        r.observe(Area::Accounts, Said::Off, true);
        assert_eq!(r.offer(), [], "the user turned it off here");
        r.chose(Area::Accounts, true);
        r.observe(Area::Accounts, Said::Off, false);
        assert_eq!(r.offer(), [Area::Accounts]);
        r.dismiss();
        assert_eq!(r.offer(), []);
        r.observe(Area::Accounts, Said::Off, true);
        assert_eq!(r.offer(), [], "dismissed");
        r.observe(Area::Accounts, Said::On, true);
        r.observe(Area::Accounts, Said::Off, true);
        assert_eq!(r.offer(), [Area::Accounts], "on again, then a restart");
        assert_eq!(Restart::default().confirm(), None);
    }
}
