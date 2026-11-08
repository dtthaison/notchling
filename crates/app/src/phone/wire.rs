//! The phone bridge's daemon ops (Milestone 4, Task 2) as the panel sends and reads them.
//!
//! The names are the plan's (`docs/superpowers/plans/2026-11-08-phase7-m4-phone.md`, Task 2).
//! The plan fixes the names and what each returns in words, not the JSON, so the readers here
//! take what the daemon sends tolerantly: snake_case or camelCase keys, a state as a word or as
//! a flag, a time in milliseconds or seconds. A field that is missing is left out of the
//! answer; it never fails the whole read.

use serde_json::{json, Value};

/// The bridge ops (app tier).
pub mod ops {
    pub const BRIDGE_SET: &str = "bridge.set";
    pub const BRIDGE_STATUS: &str = "bridge.status";
    pub const BRIDGE_PAIR_BEGIN: &str = "bridge.pair.begin";
    pub const BRIDGE_DEVICES: &str = "bridge.devices";
    pub const BRIDGE_REVOKE: &str = "bridge.revoke";
    pub const BRIDGE_TUNNEL_STATUS: &str = "bridge.tunnel.status";
}

/// One op and its arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub op: &'static str,
    pub args: Value,
}

impl Call {
    fn new(op: &'static str, args: Value) -> Call {
        Call { op, args }
    }

    pub fn set(on: bool) -> Call {
        Call::new(ops::BRIDGE_SET, json!({ "on": on }))
    }

    pub fn status() -> Call {
        Call::new(ops::BRIDGE_STATUS, json!({}))
    }

    pub fn pair_begin() -> Call {
        Call::new(ops::BRIDGE_PAIR_BEGIN, json!({}))
    }

    pub fn devices() -> Call {
        Call::new(ops::BRIDGE_DEVICES, json!({}))
    }

    pub fn revoke(device: &str) -> Call {
        Call::new(ops::BRIDGE_REVOKE, json!({ "device": device }))
    }

    pub fn tunnel_status() -> Call {
        Call::new(ops::BRIDGE_TUNNEL_STATUS, json!({}))
    }
}

/// What the bridge is doing, as the daemon says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BridgeState {
    Off,
    Starting,
    /// Listening; the port when the daemon names it.
    On(Option<u16>),
    Failed(String),
}

impl BridgeState {
    pub fn is_on(&self) -> bool {
        matches!(self, BridgeState::On(_))
    }
}

/// `bridge.status` and `bridge.set`'s answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub state: BridgeState,
    /// The certificate's SHA-256 fingerprint, once the daemon has read it.
    pub fingerprint: Option<String>,
}

/// A live pairing code and the QR payload that carries it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pairing {
    pub code: String,
    /// What the QR encodes, verbatim.
    pub payload: String,
    /// Where the phone can reach this Mac, one per line.
    pub hosts: Vec<String>,
    pub expires_at_ms: i64,
}

/// One paired phone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub paired_at_ms: Option<i64>,
    pub last_seen_ms: Option<i64>,
}

/// Where the tunnel is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunnelState {
    /// The user hasn't turned the tunnel on.
    Off,
    /// On, but a field Cloudflare Access needs is missing.
    NeedsSetup,
    /// On and configured; nothing has come through yet.
    Waiting,
    /// On, and a connection has come through.
    Up,
}

/// `bridge.tunnel.status`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tunnel {
    pub state: TunnelState,
    pub last_connection_ms: Option<i64>,
}

fn field<'a>(v: &'a Value, names: &[&str]) -> Option<&'a Value> {
    names
        .iter()
        .find_map(|n| v.get(*n))
        .filter(|v| !v.is_null())
}

fn text(v: &Value, names: &[&str]) -> Option<String> {
    field(v, names)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|s| !s.is_empty())
}

fn flag(v: &Value, names: &[&str]) -> Option<bool> {
    field(v, names).and_then(Value::as_bool)
}

/// A time as milliseconds: a number below 100 billion is seconds (that is the year 5138 in
/// milliseconds, and 1973 in seconds).
fn millis(v: &Value, names: &[&str]) -> Option<i64> {
    let n = field(v, names)?;
    let n = n
        .as_i64()
        .or_else(|| n.as_f64().map(|f| f.round() as i64))?;
    if n <= 0 {
        None
    } else if n < 100_000_000_000 {
        Some(n * 1000)
    } else {
        Some(n)
    }
}

/// Reads `bridge.status` / `bridge.set`. `None` when the answer says nothing about the bridge.
pub fn parse_status(v: &Value) -> Option<Status> {
    let port = field(v, &["port"])
        .and_then(Value::as_u64)
        .and_then(|p| u16::try_from(p).ok());
    let word = text(v, &["state", "status"]).map(|s| s.to_ascii_lowercase());
    let error = text(v, &["error", "message"]);
    let state = match word.as_deref() {
        Some("off" | "stopped" | "disabled") => BridgeState::Off,
        Some("starting") => BridgeState::Starting,
        Some("on" | "running" | "listening") => BridgeState::On(port),
        Some("failed" | "error") => {
            BridgeState::Failed(error.unwrap_or_else(|| "the bridge failed to start".into()))
        }
        Some(_) => return None,
        None => match flag(v, &["on", "running", "enabled", "bridge"])? {
            true => BridgeState::On(port),
            false => match error {
                Some(message) => BridgeState::Failed(message),
                None => BridgeState::Off,
            },
        },
    };
    Some(Status {
        state,
        fingerprint: text(v, &["fingerprint", "fp"]),
    })
}

/// Reads `bridge.pair.begin`. A reply without a code or a payload is no pairing.
pub fn parse_pairing(v: &Value) -> Option<Pairing> {
    let hosts = field(v, &["hosts"])
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Some(Pairing {
        code: text(v, &["code"])?,
        payload: text(v, &["payload", "qr"])?,
        hosts,
        expires_at_ms: millis(v, &["expires_at", "expiresAt"])?,
    })
}

/// Reads `bridge.devices`: `{devices: [...]}` or the array itself. A row without an id is
/// skipped, since Revoke could not name it.
pub fn parse_devices(v: &Value) -> Vec<Device> {
    let rows = v
        .get("devices")
        .and_then(Value::as_array)
        .or_else(|| v.as_array());
    rows.map(|rows| {
        rows.iter()
            .filter_map(|d| {
                Some(Device {
                    id: text(d, &["id", "device"])?,
                    name: text(d, &["name"]).unwrap_or_else(|| "Phone".into()),
                    platform: text(d, &["platform"]).unwrap_or_default(),
                    paired_at_ms: millis(d, &["paired_at", "pairedAt"]),
                    last_seen_ms: millis(d, &["last_seen", "lastSeen"]),
                })
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Reads `bridge.tunnel.status`: a `state` word, or `enabled`, `configured` and `connected`
/// flags. `None` when it says neither.
pub fn parse_tunnel(v: &Value) -> Option<Tunnel> {
    let last_connection_ms = millis(v, &["last_connection", "lastConnection"]);
    let state = match text(v, &["state", "status"])
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("off" | "disabled") => TunnelState::Off,
        Some("needs_setup" | "incomplete" | "unconfigured") => TunnelState::NeedsSetup,
        Some("waiting" | "idle") => TunnelState::Waiting,
        Some("up" | "connected" | "on") => TunnelState::Up,
        Some(_) => return None,
        None => {
            let enabled = flag(v, &["enabled", "on"])?;
            let configured = flag(v, &["configured", "complete"]).unwrap_or(true);
            let connected = flag(v, &["connected", "up"]).unwrap_or(last_connection_ms.is_some());
            match (enabled, configured, connected) {
                (false, _, _) => TunnelState::Off,
                (true, false, _) => TunnelState::NeedsSetup,
                (true, true, false) => TunnelState::Waiting,
                (true, true, true) => TunnelState::Up,
            }
        }
    };
    Some(Tunnel {
        state,
        last_connection_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_have_the_plan_shapes() {
        assert_eq!(Call::set(true).op, "bridge.set");
        assert_eq!(Call::set(true).args, json!({"on": true}));
        assert_eq!(Call::set(false).args, json!({"on": false}));
        assert_eq!(Call::status().op, "bridge.status");
        assert_eq!(Call::pair_begin().op, "bridge.pair.begin");
        assert_eq!(Call::devices().op, "bridge.devices");
        assert_eq!(Call::tunnel_status().op, "bridge.tunnel.status");
        assert_eq!(
            Call::revoke("d-1"),
            Call {
                op: "bridge.revoke",
                args: json!({"device": "d-1"})
            }
        );
    }

    #[test]
    fn status_reads_a_word_or_a_flag() {
        let on = parse_status(&json!({"state": "running", "port": 47821, "fingerprint": "AB:CD"}));
        assert_eq!(
            on,
            Some(Status {
                state: BridgeState::On(Some(47821)),
                fingerprint: Some("AB:CD".into())
            })
        );
        let flagged = parse_status(&json!({"on": true})).map(|s| s.state);
        assert_eq!(flagged, Some(BridgeState::On(None)));
        let off = parse_status(&json!({"on": false})).map(|s| s.state);
        assert_eq!(off, Some(BridgeState::Off));
        let starting = parse_status(&json!({"state": "Starting"})).map(|s| s.state);
        assert_eq!(starting, Some(BridgeState::Starting));
        let failed = parse_status(&json!({"state": "failed", "error": "port 47821 is busy"}))
            .map(|s| s.state);
        assert_eq!(
            failed,
            Some(BridgeState::Failed("port 47821 is busy".into()))
        );
        // Off with an error is the bridge that tried and could not.
        let tried = parse_status(&json!({"on": false, "error": "port busy"})).map(|s| s.state);
        assert_eq!(tried, Some(BridgeState::Failed("port busy".into())));
    }

    #[test]
    fn status_that_says_nothing_is_none() {
        assert_eq!(parse_status(&json!({"version": "0.1.0"})), None);
        assert_eq!(parse_status(&Value::Null), None);
        assert_eq!(parse_status(&json!({"state": "sideways"})), None);
    }

    #[test]
    fn pairing_needs_a_code_a_payload_and_an_expiry() {
        let ok = json!({"code": "K7M4XQ2P", "payload": "{\"v\":1}", "expires_at": 1_800_000_300_000_i64,
                        "hosts": ["192.168.1.20", 5, "mac.local"]});
        let p = parse_pairing(&ok);
        assert_eq!(
            p,
            Some(Pairing {
                code: "K7M4XQ2P".into(),
                payload: "{\"v\":1}".into(),
                hosts: vec!["192.168.1.20".into(), "mac.local".into()],
                expires_at_ms: 1_800_000_300_000,
            })
        );
        // Seconds, camelCase.
        let secs = json!({"code": "A", "payload": "p", "expiresAt": 1_800_000_300});
        assert_eq!(
            parse_pairing(&secs).map(|p| p.expires_at_ms),
            Some(1_800_000_300_000)
        );
        for missing in ["code", "payload", "expires_at"] {
            let mut v = ok.clone();
            v.as_object_mut().map(|o| o.remove(missing));
            assert_eq!(parse_pairing(&v), None, "without {missing}");
        }
        assert_eq!(
            parse_pairing(&json!({"code": "", "payload": "p", "expires_at": 1})),
            None
        );
    }

    #[test]
    fn devices_read_an_object_or_an_array_and_skip_rows_without_an_id() {
        let rows = json!([
            {"id": "d1", "name": "Pixel", "platform": "android", "paired_at": 1_800_000_000_000_i64,
             "last_seen": 1_800_000_100},
            {"name": "no id"},
            {"id": "d2"},
        ]);
        let want = vec![
            Device {
                id: "d1".into(),
                name: "Pixel".into(),
                platform: "android".into(),
                paired_at_ms: Some(1_800_000_000_000),
                last_seen_ms: Some(1_800_000_100_000),
            },
            Device {
                id: "d2".into(),
                name: "Phone".into(),
                platform: String::new(),
                paired_at_ms: None,
                last_seen_ms: None,
            },
        ];
        assert_eq!(parse_devices(&rows), want);
        assert_eq!(parse_devices(&json!({ "devices": rows })), want);
        assert!(parse_devices(&json!({"devices": "nope"})).is_empty());
        assert!(parse_devices(&Value::Null).is_empty());
    }

    #[test]
    fn tunnel_reads_a_word_or_flags() {
        let up = parse_tunnel(&json!({"state": "up", "last_connection": 1_800_000_000_000_i64}));
        assert_eq!(
            up,
            Some(Tunnel {
                state: TunnelState::Up,
                last_connection_ms: Some(1_800_000_000_000)
            })
        );
        let cases = [
            (json!({"enabled": false}), TunnelState::Off),
            (
                json!({"enabled": true, "configured": false}),
                TunnelState::NeedsSetup,
            ),
            (
                json!({"enabled": true, "configured": true}),
                TunnelState::Waiting,
            ),
            (json!({"enabled": true, "connected": true}), TunnelState::Up),
            (
                json!({"enabled": true, "last_connection": 1_800_000_000}),
                TunnelState::Up,
            ),
        ];
        for (v, want) in cases {
            assert_eq!(parse_tunnel(&v).map(|t| t.state), Some(want), "{v}");
        }
        assert_eq!(parse_tunnel(&json!({})), None);
    }
}
