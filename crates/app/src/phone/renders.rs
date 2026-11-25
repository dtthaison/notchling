use super::approval::PairRequest;
use super::bridge::Snapshot;
use super::wire::{BridgeState, Device, Pairing, Tunnel, TunnelState};
use crate::views::ViewSpec;

pub const NOW_MS: i64 = 1_800_000_000_000;

pub const PAIR_REQUEST: &str = "phone-pair-request";

pub const VIEWS: [ViewSpec; 6] = [
    ViewSpec {
        name: "phone-panel",
        size: (400.0, 800.0),
        about:
            "the Phone panel, bridge on with a code on show and two paired phones (made-up data)",
    },
    ViewSpec {
        name: "phone-panel-off",
        size: (400.0, 640.0),
        about: "the Phone panel with the bridge off: only the switch is live (made-up data)",
    },
    ViewSpec {
        name: "phone-panel-unpaired",
        size: (400.0, 640.0),
        about: "the Phone panel with the bridge on and no phones paired (made-up data)",
    },
    ViewSpec {
        name: "phone-panel-tunnel",
        size: (400.0, 640.0),
        about: "the Phone panel with the tunnel up and a recent connection (made-up data)",
    },
    ViewSpec {
        name: "phone-panel-detached",
        size: (400.0, 640.0),
        about: "the Phone panel with no daemon attached: every control disabled (made-up data)",
    },
    ViewSpec {
        name: PAIR_REQUEST,
        size: super::approval::PROMPT_SIZE,
        about: "the Allow/Deny prompt for a phone that gave the right code, one more waiting (made-up data)",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Paired,
    Off,
    Unpaired,
    Tunnel,
    Detached,
}

pub fn shown(name: &str) -> Option<Shown> {
    Some(match name {
        "phone-panel" => Shown::Paired,
        "phone-panel-off" => Shown::Off,
        "phone-panel-unpaired" => Shown::Unpaired,
        "phone-panel-tunnel" => Shown::Tunnel,
        "phone-panel-detached" => Shown::Detached,
        _ => return None,
    })
}

const FINGERPRINT: &str = "3F:A1:09:C4:7B:52:E8:1D:90:6A:B3:F7:2C:5E:84:D0:19:AE:63:7F:B2:48:C5:0E:9D:31:F6:8A:57:E2:1B:A4";

fn phones() -> Vec<Device> {
    vec![
        Device {
            id: "dev-pixel".into(),
            name: "Pixel 9".into(),
            platform: "android".into(),
            paired_at_ms: Some(NOW_MS - 3 * 86_400_000),
            last_seen_ms: Some(NOW_MS - 4 * 60_000),
        },
        Device {
            id: "dev-iphone".into(),
            name: "Example iPhone".into(),
            platform: "ios".into(),
            paired_at_ms: Some(NOW_MS - 20 * 86_400_000),
            last_seen_ms: None,
        },
    ]
}

pub fn pair_requests() -> Vec<PairRequest> {
    vec![
        PairRequest {
            id: "pair-1".into(),
            name: "Pixel 9".into(),
            platform: "android".into(),
            address: "192.168.1.42".into(),
        },
        PairRequest {
            id: "pair-2".into(),
            name: "Example iPhone".into(),
            platform: "ios".into(),
            address: "192.168.1.57".into(),
        },
    ]
}

fn code() -> Pairing {
    let payload = format!(
        "{{\"code\":\"K7M4XQ2P\",\"fp\":\"{FINGERPRINT}\",\"hosts\":[\"192.168.1.20\",\"studio.local\"],\
         \"name\":\"Studio Mac\",\"port\":47821,\"v\":1}}"
    );
    Pairing {
        code: "K7M4XQ2P".into(),
        payload,
        hosts: vec!["192.168.1.20".into(), "studio.local".into()],
        expires_at_ms: NOW_MS + 4 * 60_000 + 32_000,
    }
}

pub fn snapshot(shown: Shown) -> Snapshot {
    let on = Snapshot {
        attached: true,
        state: BridgeState::On(Some(47821)),
        fingerprint: Some(FINGERPRINT.into()),
        tunnel: Some(Tunnel {
            state: TunnelState::Off,
            last_connection_ms: None,
        }),
        ..Snapshot::detached()
    };
    match shown {
        Shown::Detached => Snapshot::detached(),
        Shown::Off => Snapshot {
            state: BridgeState::Off,
            pairing: None,
            devices: phones(),
            fingerprint: None,
            ..on
        },
        Shown::Unpaired => on,
        Shown::Paired => Snapshot {
            pairing: Some(code()),
            devices: phones(),
            ..on
        },
        Shown::Tunnel => Snapshot {
            devices: phones(),
            tunnel: Some(Tunnel {
                state: TunnelState::Up,
                last_connection_ms: Some(NOW_MS - 2 * 60_000),
            }),
            ..on
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phone::panel::{Pair, Panel};

    #[test]
    fn every_view_has_its_state() {
        for spec in VIEWS.iter().filter(|s| s.name != PAIR_REQUEST) {
            assert!(shown(spec.name).is_some(), "{}", spec.name);
        }
        assert!(crate::views::spec(PAIR_REQUEST).is_some());
        assert_eq!(shown(PAIR_REQUEST), None, "a prompt, not a panel");
        assert_eq!(shown("chat"), None);
        assert!(crate::views::spec("phone-panel").is_some());
    }

    #[test]
    fn the_states_show_what_their_names_say() {
        let panel = |s| Panel::new(&snapshot(s), NOW_MS);
        let paired = panel(Shown::Paired);
        assert!(matches!(paired.pair, Pair::Showing { qr: Ok(_), .. }));
        assert_eq!(paired.devices.rows.len(), 2);
        let off = panel(Shown::Off);
        assert!(!off.switch.on && off.switch.control.enabled);
        assert!(off.devices.rows.iter().all(|r| !r.revoke.enabled));
        let unpaired = panel(Shown::Unpaired);
        assert!(unpaired.devices.rows.is_empty());
        assert!(matches!(unpaired.pair, Pair::Idle { button } if button.enabled));
        let tunnel = panel(Shown::Tunnel);
        assert_eq!(tunnel.tunnel.status, "Tunnel up");
        let detached = panel(Shown::Detached);
        assert!(!detached.switch.control.enabled);
    }
}
