//! What the app knows about the phone bridge, and what it asks the daemon (Milestone 4, U6).
//!
//! The daemon ships with the bridge off: it binds no port until the app sends `bridge.set
//! {on: true}`, and it comes back off at every start. The app never turns it back on by itself:
//! [`Bridge::refresh`] only reads. The user's choice (`bridgeOn` in the app's own `settings.json`)
//! decides only whether the restart notice offers it ([`crate::switches::restart`]); only
//! [`Bridge::choose`], from the Phone panel's switch or Settings' switch, sends
//! `bridge.set`.
//!
//! Every call blocks, so the caller makes it off the UI thread. A [`Bridge`] keeps a
//! [`Snapshot`] of what the daemon last said; the panel draws from that and never reads a socket.
//! A render has no transport at all ([`Bridge::fixed`]).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use notchling_local_api::{ClientError, SOCKET_NAME};
use serde_json::Value;

use super::wire::{
    parse_devices, parse_pairing, parse_status, parse_tunnel, BridgeState, Call, Device, Pairing,
    Tunnel,
};
use crate::acting::{self, Acting};
use crate::actions::{Choice, Socket, Transport};
use crate::chat::data::DataRoot;
use crate::switches::restart::{Area, RestartNotice, Said};

/// The key in the app's `settings.json` that holds the user's choice. Like `actingOn`, it isn't in
/// the engine's key list, so it is written past the store's key check.
pub const BRIDGE_CHOICE_KEY: &str = "bridgeOn";

/// What a control says while there is no daemon to ask.
pub const DETACHED_MESSAGE: &str =
    "Notchling's daemon isn't attached, so the phone bridge can't be reached";
/// What a control says while the bridge is off.
pub const OFF_MESSAGE: &str = "The phone bridge is off. Turn on Let phones connect first";

/// The user's choice in the app's own settings file.
pub struct SettingsChoice {
    pub file: PathBuf,
}

impl Choice for SettingsChoice {
    fn get(&self) -> bool {
        crate::welcome::own::Flags {
            file: self.file.clone(),
        }
        .get(BRIDGE_CHOICE_KEY)
    }

    fn set(&self, on: bool) -> Result<(), String> {
        crate::welcome::own::Flags {
            file: self.file.clone(),
        }
        .set_choice(BRIDGE_CHOICE_KEY, on)
    }
}

/// No choice: off, and nowhere to keep one. A render's.
struct NoChoice;

impl Choice for NoChoice {
    fn get(&self) -> bool {
        false
    }
    fn set(&self, _on: bool) -> Result<(), String> {
        Err("there is nowhere to keep the choice".into())
    }
}

/// Everything the panel draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// A daemon answered the last call.
    pub attached: bool,
    pub state: BridgeState,
    /// The certificate's fingerprint, once the daemon has it.
    pub fingerprint: Option<String>,
    /// The live pairing code, until it is closed or the bridge goes off.
    pub pairing: Option<Pairing>,
    pub devices: Vec<Device>,
    pub tunnel: Option<Tunnel>,
    /// The last failure to show, or none.
    pub error: Option<String>,
}

impl Snapshot {
    /// No daemon: nothing known.
    pub fn detached() -> Snapshot {
        Snapshot {
            attached: false,
            state: BridgeState::Off,
            fingerprint: None,
            pairing: None,
            devices: Vec::new(),
            tunnel: None,
            error: None,
        }
    }
}

pub struct Bridge {
    transport: Option<Box<dyn Transport>>,
    snapshot: Mutex<Snapshot>,
    choice: Box<dyn Choice>,
    restart: Option<Arc<RestartNotice>>,
}

/// A refusal's line: the daemon's own message, else the error's.
fn message(e: &ClientError) -> String {
    match e {
        ClientError::Api(api) => api.message.clone(),
        other => other.to_string(),
    }
}

/// Whether this error means there is no daemon to talk to.
fn detaches(e: &ClientError) -> bool {
    acting::after_error(e) == Some(Acting::Detached)
}

/// The clock the app polls and counts down by, in milliseconds since the epoch.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Bridge {
    /// Over a transport; detached until the daemon answers.
    pub fn over(transport: Box<dyn Transport>) -> Bridge {
        Bridge {
            transport: Some(transport),
            snapshot: Mutex::new(Snapshot::detached()),
            choice: Box::new(NoChoice),
            restart: None,
        }
    }

    /// Keeps the user's choice in `choice`.
    pub fn with_choice(mut self, choice: Box<dyn Choice>) -> Bridge {
        self.choice = choice;
        self
    }

    /// Tells `notice` what the daemon says about the bridge and what the user chose.
    pub fn with_restart(mut self, notice: Arc<RestartNotice>) -> Bridge {
        self.restart = Some(notice);
        self
    }

    /// No daemon behind it, showing `snapshot` as given: a render's.
    pub fn fixed(snapshot: Snapshot) -> Bridge {
        Bridge {
            transport: None,
            snapshot: Mutex::new(snapshot),
            choice: Box::new(NoChoice),
            restart: None,
        }
    }

    /// The running app's: the daemon's socket in the data folder, or nothing for a fixture root,
    /// which never touches a daemon.
    pub fn for_run(root: Option<&DataRoot>) -> Bridge {
        match root.filter(|r| r.fixture.is_none()) {
            Some(root) => Bridge::over(Box::new(Socket {
                socket: root.data_dir.join(SOCKET_NAME),
                version: env!("CARGO_PKG_VERSION").to_string(),
            }))
            .with_choice(Box::new(SettingsChoice {
                file: root.data_dir.join("settings.json"),
            })),
            None => Bridge::fixed(Snapshot::detached()),
        }
    }

    /// Whether there is a daemon to ask at all; a render's and a fixture root's has none.
    pub fn is_live(&self) -> bool {
        self.transport.is_some()
    }

    pub fn snapshot(&self) -> Snapshot {
        lock(&self.snapshot).clone()
    }

    /// Whether the user chose the bridge on.
    pub fn chosen(&self) -> bool {
        self.choice.get()
    }

    fn update(&self, f: impl FnOnce(&mut Snapshot)) {
        f(&mut lock(&self.snapshot));
    }

    /// One call. A lost daemon detaches (and clears what was known); a refusal is left to the caller.
    fn call(&self, call: &Call) -> Result<Value, ClientError> {
        let Some(transport) = &self.transport else {
            return Err(ClientError::Closed);
        };
        let result = transport.call(call.op, call.args.clone());
        if let Err(e) = &result {
            if detaches(e) {
                self.update(|s| *s = Snapshot::detached());
            }
        }
        result
    }

    /// Reads the bridge, the phones and the tunnel (`bridge.status`, `bridge.devices`,
    /// `bridge.tunnel.status`). Blocking. Only reads: a daemon that says off while the user chose
    /// on is offered back by the restart notice, never turned on here.
    pub fn refresh(&self) {
        if self.transport.is_none() {
            return;
        }
        let status = match self.call(&Call::status()) {
            Ok(v) => parse_status(&v),
            Err(e) => {
                // A refusal is still an answer: a daemon without the bridge is there, and says why not.
                if !detaches(&e) {
                    self.update(|s| {
                        s.attached = true;
                        s.error = Some(message(&e));
                    });
                }
                return;
            }
        };
        let Some(status) = status else {
            self.update(|s| {
                s.attached = true;
                s.error = Some("The daemon's bridge status isn't one this app reads".into());
            });
            return;
        };
        if let Some(notice) = &self.restart {
            let said = match status.state {
                BridgeState::Off => Said::Off,
                BridgeState::On(_) | BridgeState::Starting => Said::On,
                // A bridge that failed to start says nothing about a restart.
                BridgeState::Failed(_) => Said::Detached,
            };
            notice.observe(Area::Bridge, said, self.choice.get());
        }
        let mut error = None;
        let on = status.state.is_on();
        // The lists are readable while the bridge is off (they only read the pairing files); a
        // refusal there matters only while it is on.
        let devices = self.call(&Call::devices());
        let tunnel = self.call(&Call::tunnel_status());
        if devices.as_ref().err().is_some_and(detaches)
            || tunnel.as_ref().err().is_some_and(detaches)
        {
            return;
        }
        if error.is_none() && on {
            error = devices
                .as_ref()
                .err()
                .or(tunnel.as_ref().err())
                .map(message);
        }
        self.update(|s| {
            s.attached = true;
            s.state = status.state;
            if status.fingerprint.is_some() {
                s.fingerprint = status.fingerprint;
            }
            if let Ok(v) = &devices {
                s.devices = parse_devices(v);
            }
            if let Ok(v) = &tunnel {
                s.tunnel = parse_tunnel(v);
            }
            if !on {
                s.pairing = None;
            }
            s.error = error;
        });
    }

    /// The user flipped the switch in this app: tells the daemon, then keeps the choice in the
    /// app's settings (the daemon first, so "off" always reaches it). Blocking. Turning it off
    /// also drops the pairing code on show.
    pub fn choose(&self, on: bool) -> Result<(), String> {
        if self.transport.is_none() {
            return Err(DETACHED_MESSAGE.into());
        }
        match self.call(&Call::set(on)) {
            Ok(v) => {
                if let Some(notice) = &self.restart {
                    notice.chose(Area::Bridge, on);
                }
                if let Err(e) = self.choice.set(on) {
                    eprintln!("bridge: the choice isn't kept ({e})");
                }
                let status = parse_status(&v);
                self.update(|s| {
                    s.attached = true;
                    s.error = None;
                    match status {
                        Some(status) => s.state = status.state,
                        // An answer that doesn't say: what was asked for.
                        None => {
                            s.state = if on {
                                BridgeState::Starting
                            } else {
                                BridgeState::Off
                            }
                        }
                    }
                    if !s.state.is_on() {
                        s.pairing = None;
                    }
                });
                Ok(())
            }
            Err(e) => {
                let line = if detaches(&e) {
                    DETACHED_MESSAGE.to_owned()
                } else {
                    message(&e)
                };
                if !detaches(&e) {
                    self.update(|s| s.error = Some(line.clone()));
                }
                Err(line)
            }
        }
    }

    /// Asks for a new pairing code (`bridge.pair.begin`). Refused here, unsent, while the bridge
    /// isn't on.
    pub fn begin_pairing(&self) -> Result<(), String> {
        self.guard_on()?;
        match self.call(&Call::pair_begin()) {
            Ok(v) => match parse_pairing(&v) {
                Some(pairing) => {
                    self.update(|s| {
                        s.pairing = Some(pairing);
                        s.error = None;
                    });
                    Ok(())
                }
                None => self.fail("The daemon sent a pairing code this app can't read".into()),
            },
            Err(e) => self.refused(&e),
        }
    }

    /// Stops showing the code. The daemon's code lapses on its own.
    pub fn close_pairing(&self) {
        self.update(|s| s.pairing = None);
    }

    /// Revokes a paired phone (`bridge.revoke`). Refused here, unsent, while the bridge isn't on.
    pub fn revoke(&self, device: &str) -> Result<(), String> {
        self.guard_on()?;
        match self.call(&Call::revoke(device)) {
            Ok(_) => {
                self.update(|s| {
                    s.devices.retain(|d| d.id != device);
                    s.error = None;
                });
                Ok(())
            }
            Err(e) => self.refused(&e),
        }
    }

    fn guard_on(&self) -> Result<(), String> {
        let s = self.snapshot();
        if self.transport.is_none() || !s.attached {
            Err(DETACHED_MESSAGE.into())
        } else if !s.state.is_on() {
            Err(OFF_MESSAGE.into())
        } else {
            Ok(())
        }
    }

    fn fail(&self, line: String) -> Result<(), String> {
        self.update(|s| s.error = Some(line.clone()));
        Err(line)
    }

    fn refused(&self, e: &ClientError) -> Result<(), String> {
        if detaches(e) {
            Err(DETACHED_MESSAGE.into())
        } else {
            self.fail(message(e))
        }
    }
}

#[cfg(test)]
#[path = "bridge_tests.rs"]
mod tests;
