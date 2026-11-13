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

pub const BRIDGE_CHOICE_KEY: &str = "bridgeOn";

pub const DETACHED_MESSAGE: &str =
    "Notchling's daemon isn't attached, so the phone bridge can't be reached";
pub const OFF_MESSAGE: &str = "The phone bridge is off. Turn on Let phones connect first";

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

struct NoChoice;

impl Choice for NoChoice {
    fn get(&self) -> bool {
        false
    }
    fn set(&self, _on: bool) -> Result<(), String> {
        Err("there is nowhere to keep the choice".into())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub attached: bool,
    pub state: BridgeState,
    pub fingerprint: Option<String>,
    pub pairing: Option<Pairing>,
    pub devices: Vec<Device>,
    pub tunnel: Option<Tunnel>,
    pub error: Option<String>,
}

impl Snapshot {
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

fn message(e: &ClientError) -> String {
    match e {
        ClientError::Api(api) => api.message.clone(),
        other => other.to_string(),
    }
}

fn detaches(e: &ClientError) -> bool {
    acting::after_error(e) == Some(Acting::Detached)
}

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
    pub fn over(transport: Box<dyn Transport>) -> Bridge {
        Bridge {
            transport: Some(transport),
            snapshot: Mutex::new(Snapshot::detached()),
            choice: Box::new(NoChoice),
            restart: None,
        }
    }

    pub fn with_choice(mut self, choice: Box<dyn Choice>) -> Bridge {
        self.choice = choice;
        self
    }

    pub fn with_restart(mut self, notice: Arc<RestartNotice>) -> Bridge {
        self.restart = Some(notice);
        self
    }

    pub fn fixed(snapshot: Snapshot) -> Bridge {
        Bridge {
            transport: None,
            snapshot: Mutex::new(snapshot),
            choice: Box::new(NoChoice),
            restart: None,
        }
    }

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

    pub fn is_live(&self) -> bool {
        self.transport.is_some()
    }

    pub fn snapshot(&self) -> Snapshot {
        lock(&self.snapshot).clone()
    }

    pub fn chosen(&self) -> bool {
        self.choice.get()
    }

    fn update(&self, f: impl FnOnce(&mut Snapshot)) {
        f(&mut lock(&self.snapshot));
    }

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

    pub fn refresh(&self) {
        if self.transport.is_none() {
            return;
        }
        let status = match self.call(&Call::status()) {
            Ok(v) => parse_status(&v),
            Err(e) => {
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
                BridgeState::Failed(_) => Said::Detached,
            };
            notice.observe(Area::Bridge, said, self.choice.get());
        }
        let mut error = None;
        let on = status.state.is_on();
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

    pub fn close_pairing(&self) {
        self.update(|s| s.pairing = None);
    }

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
