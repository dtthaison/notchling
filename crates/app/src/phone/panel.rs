//! What the Phone panel shows, pure (part U6, the Swift `PhonePanel`): the bridge switch, the
//! pairing code and QR, the paired phones with Revoke, and the tunnel's status, each control with
//! the reason it is disabled when it is. Drawn from a [`Snapshot`] and the clock; the GPUI view
//! (`view`, macOS) only lays it out.

use super::bridge::{Snapshot, DETACHED_MESSAGE, OFF_MESSAGE};
use super::qr::Qr;
use super::wire::{BridgeState, Device, Pairing, Tunnel, TunnelState};
use crate::acting::Control;
use crate::chat::format::age;

/// Where cloudflared points, shown under the tunnel (the Swift app shows the same line).
pub const TUNNEL_ORIGIN: &str = "cloudflared → http://127.0.0.1:47822";
/// The switch's explanation.
pub const SWITCH_NOTE: &str = "Off by default. Accepts this Mac, the private LAN and Tailscale over TLS with a pinned fingerprint.";

/// The dot beside the bridge's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dot {
    Off,
    On,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusLine {
    pub word: String,
    pub dot: Dot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Switch {
    pub on: bool,
    pub control: Control,
}

/// The pairing section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pair {
    /// No code on show: the Pair a Phone button.
    Idle { button: Control },
    Showing {
        /// The code as a grid, or why it can't be drawn.
        qr: Result<Qr, String>,
        code: String,
        /// "Expires in 4:32", or "Expired".
        expires: String,
        expired: bool,
        /// Where the phone can reach this Mac, one per line.
        hosts: Vec<String>,
        new_code: Control,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceRow {
    pub id: String,
    pub name: String,
    /// "ios · paired 3d ago · last seen 5m ago".
    pub detail: String,
    pub revoke: Control,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Devices {
    /// "Paired phones (2)".
    pub title: String,
    pub rows: Vec<DeviceRow>,
    /// Shown instead of rows when there are none.
    pub empty: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TunnelBlock {
    pub status: String,
    pub dot: Dot,
    /// "Last connection 4m ago", while there was one.
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Panel {
    pub status: StatusLine,
    pub switch: Switch,
    pub pair: Pair,
    pub devices: Devices,
    pub tunnel: TunnelBlock,
    /// The certificate's SHA-256 fingerprint, once the daemon has it.
    pub fingerprint: Option<String>,
    pub error: Option<String>,
}

/// Why a control that needs the bridge is off, or `None` while it is live.
pub fn gate(s: &Snapshot) -> Control {
    let reason = if !s.attached {
        Some(DETACHED_MESSAGE)
    } else if !s.state.is_on() {
        Some(OFF_MESSAGE)
    } else {
        None
    };
    Control {
        enabled: reason.is_none(),
        tooltip: reason,
    }
}

fn status_line(s: &Snapshot) -> StatusLine {
    let (word, dot) = if !s.attached {
        ("Daemon not attached".to_owned(), Dot::Off)
    } else {
        match &s.state {
            BridgeState::Off => ("Off".to_owned(), Dot::Off),
            BridgeState::Starting => ("Starting…".to_owned(), Dot::Off),
            BridgeState::On(Some(port)) => (format!("On, port {port}"), Dot::On),
            BridgeState::On(None) => ("On".to_owned(), Dot::On),
            BridgeState::Failed(message) => (format!("Error: {message}"), Dot::Error),
        }
    };
    StatusLine { word, dot }
}

/// "5m ago", "just now"; "unknown" without a time.
fn ago(now_ms: i64, then_ms: Option<i64>) -> String {
    match then_ms.map(|t| age(now_ms, t)) {
        Some(a) if a == "now" => "just now".to_owned(),
        Some(a) if !a.is_empty() => format!("{a} ago"),
        _ => "unknown".to_owned(),
    }
}

fn pair(s: &Snapshot, now_ms: i64) -> Pair {
    let control = gate(s);
    match &s.pairing {
        Some(p) if s.state.is_on() => showing(p, now_ms, control),
        _ => Pair::Idle { button: control },
    }
}

fn showing(p: &Pairing, now_ms: i64, control: Control) -> Pair {
    let left = (p.expires_at_ms - now_ms) / 1000;
    let expired = left <= 0;
    Pair::Showing {
        qr: Qr::new(&p.payload),
        code: p.code.clone(),
        expires: if expired {
            "Expired".to_owned()
        } else {
            format!("Expires in {}:{:02}", left / 60, left % 60)
        },
        expired,
        hosts: p.hosts.clone(),
        new_code: control,
    }
}

fn device_row(d: &Device, now_ms: i64, control: Control) -> DeviceRow {
    let seen = match d.last_seen_ms {
        Some(t) => ago(now_ms, Some(t)),
        None => "never".to_owned(),
    };
    let mut parts = Vec::new();
    if !d.platform.is_empty() {
        parts.push(d.platform.clone());
    }
    parts.push(format!("paired {}", ago(now_ms, d.paired_at_ms)));
    parts.push(format!("last seen {seen}"));
    DeviceRow {
        id: d.id.clone(),
        name: d.name.clone(),
        detail: parts.join(" · "),
        revoke: control,
    }
}

fn tunnel(s: &Snapshot, now_ms: i64) -> TunnelBlock {
    let unknown = |status: &str| TunnelBlock {
        status: status.to_owned(),
        dot: Dot::Off,
        detail: None,
    };
    if !s.attached {
        return unknown("Not available without the daemon");
    }
    let Some(Tunnel {
        state,
        last_connection_ms,
    }) = &s.tunnel
    else {
        return unknown("Tunnel status unavailable");
    };
    let (status, dot) = match state {
        TunnelState::Off => ("Tunnel off", Dot::Off),
        TunnelState::NeedsSetup => (
            "Tunnel on, but Cloudflare Access isn't fully set up",
            Dot::Error,
        ),
        TunnelState::Waiting => ("Tunnel on. No connection yet", Dot::Off),
        TunnelState::Up => ("Tunnel up", Dot::On),
    };
    TunnelBlock {
        status: status.to_owned(),
        dot,
        detail: last_connection_ms.map(|t| format!("Last connection {}", ago(now_ms, Some(t)))),
    }
}

impl Panel {
    pub fn new(s: &Snapshot, now_ms: i64) -> Panel {
        let control = gate(s);
        let rows: Vec<DeviceRow> = s
            .devices
            .iter()
            .map(|d| device_row(d, now_ms, control))
            .collect();
        Panel {
            status: status_line(s),
            switch: Switch {
                on: s.attached && matches!(s.state, BridgeState::On(_) | BridgeState::Starting),
                control: Control {
                    enabled: s.attached,
                    tooltip: (!s.attached).then_some(DETACHED_MESSAGE),
                },
            },
            pair: pair(s, now_ms),
            devices: Devices {
                title: format!("Paired phones ({})", rows.len()),
                empty: rows.is_empty().then_some("No phones yet."),
                rows,
            },
            tunnel: tunnel(s, now_ms),
            fingerprint: s.fingerprint.clone().filter(|_| s.attached),
            error: s.error.clone(),
        }
    }

    /// Whether the clock matters to what's drawn: a code counts down.
    pub fn ticking(&self) -> bool {
        matches!(&self.pair, Pair::Showing { expired: false, .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    fn on() -> Snapshot {
        Snapshot {
            attached: true,
            state: BridgeState::On(Some(47821)),
            ..Snapshot::detached()
        }
    }

    fn off() -> Snapshot {
        Snapshot {
            attached: true,
            ..Snapshot::detached()
        }
    }

    fn phone(id: &str, name: &str, platform: &str) -> Device {
        Device {
            id: id.into(),
            name: name.into(),
            platform: platform.into(),
            paired_at_ms: Some(NOW - 3 * 86_400_000),
            last_seen_ms: Some(NOW - 5 * 60_000),
        }
    }

    fn code(expires_in_s: i64) -> Pairing {
        Pairing {
            code: "K7M4XQ2P".into(),
            payload: "{\"v\":1,\"code\":\"K7M4XQ2P\"}".into(),
            hosts: vec!["192.168.1.20".into()],
            expires_at_ms: NOW + expires_in_s * 1000,
        }
    }

    #[test]
    fn without_a_daemon_every_control_says_so() {
        let p = Panel::new(&Snapshot::detached(), NOW);
        assert_eq!(p.status.word, "Daemon not attached");
        assert!(!p.switch.on);
        assert!(!p.switch.control.enabled);
        assert_eq!(p.switch.control.tooltip, Some(DETACHED_MESSAGE));
        assert_eq!(
            p.pair,
            Pair::Idle {
                button: Control {
                    enabled: false,
                    tooltip: Some(DETACHED_MESSAGE)
                }
            }
        );
        assert_eq!(p.tunnel.status, "Not available without the daemon");
        assert_eq!(p.fingerprint, None);
    }

    #[test]
    fn while_the_bridge_is_off_only_the_switch_is_live() {
        let mut s = off();
        s.devices = vec![phone("d1", "Pixel", "android")];
        let p = Panel::new(&s, NOW);
        assert_eq!(
            p.status,
            StatusLine {
                word: "Off".into(),
                dot: Dot::Off
            }
        );
        assert!(p.switch.control.enabled && p.switch.control.tooltip.is_none());
        assert!(!p.switch.on);
        let off_control = Control {
            enabled: false,
            tooltip: Some(OFF_MESSAGE),
        };
        assert_eq!(
            p.pair,
            Pair::Idle {
                button: off_control
            }
        );
        assert_eq!(p.devices.rows[0].revoke, off_control);
    }

    #[test]
    fn on_with_no_phones_offers_pairing() {
        let p = Panel::new(&on(), NOW);
        assert_eq!(
            p.status,
            StatusLine {
                word: "On, port 47821".into(),
                dot: Dot::On
            }
        );
        assert!(p.switch.on && p.switch.control.enabled);
        assert_eq!(
            p.pair,
            Pair::Idle {
                button: Control {
                    enabled: true,
                    tooltip: None
                }
            }
        );
        assert_eq!(p.devices.title, "Paired phones (0)");
        assert_eq!(p.devices.empty, Some("No phones yet."));
        assert!(p.devices.rows.is_empty());
    }

    #[test]
    fn a_pairing_shows_its_code_qr_and_countdown() {
        let mut s = on();
        s.pairing = Some(code(272));
        let p = Panel::new(&s, NOW);
        let Pair::Showing {
            qr,
            code,
            expires,
            expired,
            hosts,
            new_code,
        } = &p.pair
        else {
            panic!("expected a code");
        };
        assert_eq!(code, "K7M4XQ2P");
        assert_eq!(expires, "Expires in 4:32");
        assert!(!expired && new_code.enabled);
        assert_eq!(hosts, &["192.168.1.20"]);
        assert!(qr.as_ref().is_ok_and(|q| q.width() >= 21));
        assert!(p.ticking());
    }

    #[test]
    fn an_expired_code_says_so_and_stops_the_clock() {
        let mut s = on();
        s.pairing = Some(code(-1));
        let p = Panel::new(&s, NOW);
        assert!(
            matches!(&p.pair, Pair::Showing { expires, expired: true, .. } if expires == "Expired")
        );
        assert!(!p.ticking());
        // The seconds are zero-padded.
        s.pairing = Some(code(61));
        let Pair::Showing { expires, .. } = Panel::new(&s, NOW).pair else {
            panic!("expected a code");
        };
        assert_eq!(expires, "Expires in 1:01");
    }

    #[test]
    fn a_code_the_bridge_no_longer_backs_is_not_shown() {
        let mut s = off();
        s.pairing = Some(code(100));
        assert!(matches!(Panel::new(&s, NOW).pair, Pair::Idle { .. }));
    }

    #[test]
    fn a_payload_that_cannot_be_drawn_still_shows_the_code() {
        let mut s = on();
        let mut pairing = code(100);
        pairing.payload = "x".repeat(4000);
        s.pairing = Some(pairing);
        let Pair::Showing { qr, code, .. } = Panel::new(&s, NOW).pair else {
            panic!("expected a code");
        };
        assert!(qr.is_err());
        assert_eq!(code, "K7M4XQ2P");
    }

    #[test]
    fn two_paired_phones_each_have_a_live_revoke() {
        let mut s = on();
        s.devices = vec![
            phone("d1", "Pixel 9", "android"),
            phone("d2", "iPhone", "ios"),
        ];
        s.devices[1].last_seen_ms = None;
        let p = Panel::new(&s, NOW);
        assert_eq!(p.devices.title, "Paired phones (2)");
        assert_eq!(p.devices.empty, None);
        assert_eq!(
            p.devices.rows[0].detail,
            "android · paired 3d ago · last seen 5m ago"
        );
        assert_eq!(
            p.devices.rows[1].detail,
            "ios · paired 3d ago · last seen never"
        );
        assert!(p.devices.rows.iter().all(|r| r.revoke.enabled));
        assert_eq!(p.devices.rows[0].id, "d1");
    }

    #[test]
    fn a_phone_without_a_platform_or_times_still_reads_well() {
        let mut s = on();
        s.devices = vec![Device {
            id: "d9".into(),
            name: "Phone".into(),
            platform: String::new(),
            paired_at_ms: None,
            last_seen_ms: Some(NOW - 10_000),
        }];
        let p = Panel::new(&s, NOW);
        assert_eq!(
            p.devices.rows[0].detail,
            "paired unknown · last seen just now"
        );
    }

    #[test]
    fn the_tunnel_says_where_it_is() {
        let mut s = on();
        let say = |s: &Snapshot| Panel::new(s, NOW).tunnel;
        assert_eq!(say(&s).status, "Tunnel status unavailable");
        for (state, status, dot) in [
            (TunnelState::Off, "Tunnel off", Dot::Off),
            (
                TunnelState::NeedsSetup,
                "Tunnel on, but Cloudflare Access isn't fully set up",
                Dot::Error,
            ),
            (
                TunnelState::Waiting,
                "Tunnel on. No connection yet",
                Dot::Off,
            ),
            (TunnelState::Up, "Tunnel up", Dot::On),
        ] {
            s.tunnel = Some(Tunnel {
                state,
                last_connection_ms: None,
            });
            let t = say(&s);
            assert_eq!((t.status.as_str(), t.dot), (status, dot));
            assert_eq!(t.detail, None);
        }
        s.tunnel = Some(Tunnel {
            state: TunnelState::Up,
            last_connection_ms: Some(NOW - 4 * 60_000),
        });
        assert_eq!(say(&s).detail.as_deref(), Some("Last connection 4m ago"));
    }

    #[test]
    fn a_failed_bridge_is_an_error_and_the_error_line_passes_through() {
        let mut s = off();
        s.state = BridgeState::Failed("port 47821 is busy".into());
        s.error = Some("port 47821 is busy".into());
        let p = Panel::new(&s, NOW);
        assert_eq!(p.status.dot, Dot::Error);
        assert_eq!(p.status.word, "Error: port 47821 is busy");
        assert_eq!(p.error.as_deref(), Some("port 47821 is busy"));
        assert!(!p.switch.on);
        let starting = Snapshot {
            state: BridgeState::Starting,
            ..off()
        };
        assert!(Panel::new(&starting, NOW).switch.on);
    }

    #[test]
    fn the_fingerprint_shows_only_with_a_daemon() {
        let mut s = on();
        s.fingerprint = Some("AB:CD".into());
        assert_eq!(Panel::new(&s, NOW).fingerprint.as_deref(), Some("AB:CD"));
        s.attached = false;
        assert_eq!(Panel::new(&s, NOW).fingerprint, None);
    }
}
