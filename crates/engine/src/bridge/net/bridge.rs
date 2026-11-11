//! `BridgeController`: the switch and what the app asks of the bridge.
//!
//! Off is the start state. While off the bridge binds no port and writes nothing under the data
//! folder; it reads `devices.json` and `access.json` only when asked for them. On loads the Swift
//! app's identity (never making one), opens the stores and binds both ports; a busy port is an
//! error, never a reason to pick another, since phones and the QR rely on the port.
//!
//! The replies are the shapes the desktop app's pairing panel reads (`crates/app/src/phone/wire.rs`).

use super::access::AccessConfig;
use super::address::LocalAddresses;
use super::core::{Core, CoreParts};
use super::devices::{read_devices, PairedDevice};
use super::identity::BridgeIdentity;
use super::jwt::{Clock, Fetch};
use super::server::{Kind, Limits, Listener};
use crate::bridge::BridgeBackend;
use serde_json::{json, Value};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// How often the hub pushes what changed (Swift `BridgeHub`'s timer).
pub const TICK: Duration = Duration::from_millis(300);

#[derive(Clone, Debug)]
pub struct BridgeConfig {
    /// `AgentConsole` (the bridge's files are in its `bridge/`).
    pub data_dir: PathBuf,
    pub lan: SocketAddr,
    pub tunnel: SocketAddr,
    /// The computer's name in the QR.
    pub name: String,
    /// The `.local` name offered after the addresses.
    pub local_host: Option<String>,
    pub limits: Limits,
    /// `session.new` opens only folders inside it.
    pub home: PathBuf,
    /// Where phone photos are saved (`~/Library/Caches/AgentConsole/phone-images` on the Mac).
    pub images_dir: PathBuf,
}

impl BridgeConfig {
    /// The defaults: every interface on 47821, loopback on 47822.
    pub fn new(data_dir: PathBuf, home: PathBuf, images_dir: PathBuf) -> BridgeConfig {
        let host = host_name();
        BridgeConfig {
            data_dir,
            lan: SocketAddr::new(default_lan_ip(), super::ports::LAN),
            tunnel: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), super::ports::TUNNEL),
            name: host
                .as_deref()
                .map_or("Computer", |h| h.trim_end_matches(".local"))
                .to_string(),
            local_host: host.map(|h| {
                if h.ends_with(".local") {
                    h
                } else {
                    format!("{h}.local")
                }
            }),
            limits: Limits::default(),
            home,
            images_dir,
        }
    }

    /// The same folders on other ports (0: any free one, for tests).
    pub fn with_ports(mut self, lan: u16, tunnel: u16) -> BridgeConfig {
        self.lan.set_port(lan);
        self.tunnel.set_port(tunnel);
        self
    }

    pub fn bridge_dir(&self) -> PathBuf {
        self.data_dir.join("bridge")
    }
}

/// Dual-stack where the OS makes `::` take IPv4 too; Windows' `::` is IPv6 only.
fn default_lan_ip() -> IpAddr {
    if cfg!(windows) {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    } else {
        IpAddr::V6(Ipv6Addr::UNSPECIFIED)
    }
}

#[cfg(unix)]
pub fn host_name() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: gethostname writes at most `buf.len()` bytes into `buf`.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    std::str::from_utf8(&buf[..end])
        .ok()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(not(unix))]
pub fn host_name() -> Option<String> {
    std::env::var("COMPUTERNAME").ok().filter(|s| !s.is_empty())
}

struct Running {
    core: Arc<Core>,
    lan: Listener,
    tunnel: Listener,
    fingerprint: String,
    ticking: Arc<AtomicBool>,
    ticker: Option<JoinHandle<()>>,
}

enum State {
    Off,
    On(Running),
    Failed(String),
}

pub struct Bridge {
    config: BridgeConfig,
    backend: Arc<dyn BridgeBackend>,
    fetch: Fetch,
    now: Clock,
    state: Mutex<State>,
}

impl Bridge {
    /// Off.
    pub fn new(
        config: BridgeConfig,
        backend: Arc<dyn BridgeBackend>,
        fetch: Fetch,
        now: Clock,
    ) -> Bridge {
        Bridge {
            config,
            backend,
            fetch,
            now,
            state: Mutex::new(State::Off),
        }
    }

    pub fn config(&self) -> &BridgeConfig {
        &self.config
    }

    pub fn is_on(&self) -> bool {
        matches!(*self.lock(), State::On(_))
    }

    /// The running stores, for the router (`None` while off).
    pub fn core(&self) -> Option<Arc<Core>> {
        match &*self.lock() {
            State::On(running) => Some(running.core.clone()),
            _ => None,
        }
    }

    /// The first step of bridge-off, before anything is refused or stopped: from here on nothing
    /// in flight writes under the bridge folder ([`Core::halt`]).
    pub fn halt(&self) {
        if let Some(core) = self.core() {
            core.halt();
        }
    }

    /// The bound (LAN, tunnel) ports while on.
    pub fn ports(&self) -> Option<(u16, u16)> {
        match &*self.lock() {
            State::On(r) => Some((r.lan.port(), r.tunnel.port())),
            _ => None,
        }
    }

    /// `bridge.set {on}`: the status after.
    pub fn set(&self, on: bool) -> Value {
        let mut state = self.lock();
        match (on, &*state) {
            (true, State::On(_)) | (false, State::Off) => {}
            (true, _) => {
                *state = match self.start() {
                    Ok(running) => State::On(running),
                    Err(why) => State::Failed(why),
                };
            }
            (false, _) => {
                let old = std::mem::replace(&mut *state, State::Off);
                drop(state);
                if let State::On(running) = old {
                    stop(running);
                }
                return self.status();
            }
        }
        drop(state);
        self.status()
    }

    fn start(&self) -> Result<Running, String> {
        let bridge_dir = self.config.bridge_dir();
        let identity = BridgeIdentity::load(&bridge_dir).map_err(|e| e.to_string())?;
        let tls = identity.server_config().map_err(|e| e.to_string())?;
        let core = Arc::new(Core::new(
            &bridge_dir,
            CoreParts {
                backend: self.backend.clone(),
                home: self.config.home.clone(),
                images_dir: self.config.images_dir.clone(),
                fetch: self.fetch.clone(),
                now: self.now.clone(),
            },
        ));
        let limits = self.config.limits;
        let bind = |addr: SocketAddr, kind: Kind| {
            Listener::start(addr, kind, core.clone(), limits)
                .map_err(|e| format!("port {} is in use or can't be opened: {e}", addr.port()))
        };
        let lan = match bind(self.config.lan, Kind::Lan(tls.clone())) {
            Ok(l) => l,
            // No IPv6 here: every IPv4 interface instead.
            Err(_) if self.config.lan.ip() == IpAddr::V6(Ipv6Addr::UNSPECIFIED) => bind(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), self.config.lan.port()),
                Kind::Lan(tls),
            )?,
            Err(e) => return Err(e),
        };
        let tunnel = bind(self.config.tunnel, Kind::Tunnel)?;
        let ticking = Arc::new(AtomicBool::new(true));
        let ticker = {
            let (core, ticking) = (core.clone(), ticking.clone());
            std::thread::Builder::new()
                .name("bridge-hub".into())
                .spawn(move || {
                    while ticking.load(Ordering::SeqCst) {
                        std::thread::sleep(TICK);
                        core.tick();
                    }
                })
                .map_err(|e| format!("the bridge can't start its timer: {e}"))?
        };
        Ok(Running {
            core,
            lan,
            tunnel,
            fingerprint: identity.fingerprint,
            ticking,
            ticker: Some(ticker),
        })
    }

    /// `bridge.status`: `{state, port, fingerprint, tunnelPort, connections, error}`.
    pub fn status(&self) -> Value {
        match &*self.lock() {
            State::Off => json!({"state": "off", "port": null, "fingerprint": null}),
            State::Failed(why) => {
                json!({"state": "failed", "port": null, "fingerprint": null, "error": why})
            }
            State::On(r) => json!({
                "state": "on",
                "port": r.lan.port(),
                "fingerprint": r.fingerprint,
                "tunnelPort": r.tunnel.port(),
                "connections": r.core.connections(),
            }),
        }
    }

    /// `bridge.pair.begin`: a new one-time code and the QR text, `{code, payload, expires_at, hosts}`.
    /// Only while on, since the QR names the running port and certificate.
    pub fn pair_begin(&self) -> Result<Value, String> {
        let state = self.lock();
        let State::On(running) = &*state else {
            return Err("the bridge is off".into());
        };
        let issued = running
            .core
            .codes
            .issue((self.now)())
            .map_err(|e| e.to_string())?;
        let addresses = LocalAddresses::current();
        let mut hosts = addresses.lan;
        hosts.extend(addresses.tailscale);
        hosts.extend(self.config.local_host.clone());
        let payload = super::access::pairing_payload(
            &self.config.name,
            running.lan.port(),
            &running.fingerprint,
            &hosts,
            &issued.code,
            &running.core.access.config(),
        );
        Ok(
            json!({"code": issued.code, "payload": payload, "expires_at": issued.expires_at, "hosts": hosts}),
        )
    }

    /// `bridge.devices`: `{devices: [{id, name, platform, paired_at, last_seen}]}`, times in ms. While
    /// off this reads `devices.json` and writes nothing.
    pub fn devices(&self) -> Result<Value, String> {
        let list = match self.core() {
            Some(core) => core.devices.devices(),
            None => read_devices(&self.config.bridge_dir().join(super::devices::FILE_NAME))
                .map_err(|e| e.to_string())?,
        };
        Ok(json!({"devices": list.iter().map(device_json).collect::<Vec<_>>()}))
    }

    /// `bridge.revoke {device}`: only while on (off writes nothing). `{revoked: bool}`.
    pub fn revoke(&self, device: &str) -> Result<Value, String> {
        let core = self.core().ok_or("the bridge is off")?;
        let removed = core.revoke(device).map_err(|e| e.to_string())?;
        Ok(json!({"revoked": removed.is_some()}))
    }

    /// `bridge.tunnel.status`: `{enabled, configured, connected, last_connection, listening, hostname}`.
    /// Never the client secret.
    pub fn tunnel_status(&self) -> Value {
        let (config, last, listening) = match &*self.lock() {
            State::On(r) => (
                r.core.access.config(),
                r.core.last_tunnel_connection(),
                true,
            ),
            _ => (
                AccessConfig::read(&self.config.bridge_dir().join(super::access::FILE_NAME)),
                None,
                false,
            ),
        };
        json!({
            "enabled": config.enabled,
            "configured": config.is_complete(),
            "connected": listening && last.is_some(),
            "last_connection": last,
            "listening": listening,
            "hostname": config.hostname,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let old = std::mem::replace(&mut *self.lock(), State::Off);
        if let State::On(running) = old {
            stop(running);
        }
    }
}

fn stop(mut running: Running) {
    running.core.halt();
    running.core.codes.cancel();
    running.ticking.store(false, Ordering::SeqCst);
    if let Some(ticker) = running.ticker.take() {
        let _ = ticker.join();
    }
    running.lan.stop();
    running.tunnel.stop();
}

fn device_json(d: &PairedDevice) -> Value {
    json!({"id": d.id, "name": d.name, "platform": d.platform, "paired_at": d.paired_at, "last_seen": d.last_seen})
}

#[cfg(test)]
#[path = "bridge_tests.rs"]
pub(crate) mod tests;
