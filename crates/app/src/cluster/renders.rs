use serde_json::{json, Value};

use super::model::ClusterModel;
use super::{HEIGHT, WIDTH};
use crate::views::ViewSpec;

pub const ME: &str = "a1b2c3d4e5f60718";
pub const STANDBY: &str = "1122334455667788";
pub const WORKER: &str = "99aabbccddeeff00";
pub const THIS_MAC: &str = "MacBook";

pub const VIEWS: [ViewSpec; 8] = [
    ViewSpec {
        name: "cluster",
        size: (WIDTH, HEIGHT),
        about: "the cluster sheet: this Mac as Primary, a Standby online, a Worker offline with a version warning (made-up data)",
    },
    ViewSpec {
        name: "cluster-empty",
        size: (WIDTH, 440.0),
        about: "the cluster sheet before a cluster exists: Create Cluster and Join (made-up data)",
    },
    ViewSpec {
        name: "cluster-many",
        size: (WIDTH, HEIGHT),
        about: "the cluster sheet with more machines than fit, so it scrolls (made-up data)",
    },
    ViewSpec {
        name: "cluster-menu",
        size: (WIDTH, HEIGHT),
        about: "the cluster sheet with the Standby row's menu open: Install Hook is offered on a Mac with no app (made-up data)",
    },
    ViewSpec {
        name: "cluster-install",
        size: (WIDTH, HEIGHT),
        about: "the cluster sheet asking to install the hook on the headless Standby, with the plan shown first (made-up data)",
    },
    ViewSpec {
        name: "cluster-install-off",
        size: (WIDTH, HEIGHT),
        about: "the cluster sheet after Install Hook on a Standby whose Install switch is off: the line that says so (made-up data)",
    },
    ViewSpec {
        name: "cluster-off",
        size: (WIDTH, HEIGHT),
        about: "the cluster sheet while the cluster switch is off: the hint and Turn On (made-up data)",
    },
    ViewSpec {
        name: "cluster-joining",
        size: (WIDTH, 300.0),
        about: "the cluster sheet while this Mac waits for the Primary to approve its join (made-up data)",
    },
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shown {
    Full,
    Empty,
    Many,
    Menu,
    Off,
    Joining,
    Install,
    InstallOff,
}

pub fn shown(name: &str) -> Option<Shown> {
    match name {
        "cluster" => Some(Shown::Full),
        "cluster-empty" => Some(Shown::Empty),
        "cluster-many" => Some(Shown::Many),
        "cluster-menu" => Some(Shown::Menu),
        "cluster-off" => Some(Shown::Off),
        "cluster-joining" => Some(Shown::Joining),
        "cluster-install" => Some(Shown::Install),
        "cluster-install-off" => Some(Shown::InstallOff),
        _ => None,
    }
}

fn versions(os: &str, arch: &str, claude: Option<&str>, codex: &str) -> Value {
    let mut tools = json!({"codex": codex, "antigravity": "1.5.1"});
    if let Some(claude) = claude {
        tools["claude"] = json!(claude);
    }
    json!({
        "notchling": "0.9.0",
        "proto": 1,
        "features": ["dispatch"],
        "schema": 1,
        "os": os,
        "arch": arch,
        "tools": tools,
        "logins": {},
    })
}

#[allow(clippy::too_many_arguments)]
fn member(
    id: &str,
    name: &str,
    role: &str,
    can_be_master: bool,
    online: bool,
    versions: Value,
    load: Value,
    reference: bool,
) -> Value {
    json!({
        "id": id,
        "name": name,
        "role": role,
        "canBeMaster": can_be_master,
        "online": online,
        "client": false,
        "headless": role == "standby",
        "reference": reference,
        "state": "active",
        "versions": versions,
        "load": load,
        "desired": {"acting": role == "home", "bridge": role == "home", "install": false},
        "acceptsDelegation": role != "home",
    })
}

pub fn status() -> Value {
    json!({
        "cluster": {"id": "c-1", "name": "Studio", "term": {"n": 1, "node": ME}, "home": ME, "vault": ME},
        "me": ME,
        "members": [
            member(
                WORKER,
                "build-box",
                "worker",
                false,
                false,
                versions("linux", "x86_64", None, "0.166.0"),
                Value::Null,
                false,
            ),
            member(
                STANDBY,
                "mac-mini",
                "standby",
                true,
                true,
                versions("macos", "aarch64", Some("2.1.320"), "0.170.0"),
                json!({"cpu": 12, "memUsedMb": 6144, "memTotalMb": 16384}),
                false,
            ),
            member(
                ME,
                "MacBook",
                "home",
                true,
                true,
                versions("macos", "aarch64", Some("2.1.320"), "0.170.0"),
                json!({"cpu": 31, "memUsedMb": 18432, "memTotalMb": 32768}),
                true,
            ),
        ],
        "link": true,
        "listening": [47823],
        "tailscale": true,
        "error": null,
    })
}

pub fn empty() -> Value {
    json!({"cluster": null, "me": ME, "members": [], "link": false, "listening": null, "tailscale": true, "error": null})
}

pub fn off() -> Value {
    let mut status = status();
    status["link"] = json!(false);
    status["listening"] = Value::Null;
    status
}

pub fn joining(answered: bool) -> Value {
    let mut status = empty();
    status["joining"] = json!({
        "since": 1_700_000_000_000_i64,
        "cluster": "c-1",
        "primary": STANDBY,
        "answered": answered,
    });
    status
}

pub fn many() -> Value {
    let mut status = status();
    let extra: Vec<Value> = (1..=6)
        .map(|n| {
            member(
                &format!("00000000000000{n:02x}"),
                &format!("worker-{n}"),
                "worker",
                false,
                n % 2 == 0,
                versions("linux", "x86_64", None, "0.166.0"),
                Value::Null,
                false,
            )
        })
        .collect();
    if let Some(members) = status["members"].as_array_mut() {
        members.extend(extra);
    }
    status
}

pub fn model(full: bool) -> ClusterModel {
    ClusterModel::from_status(&if full { status() } else { empty() }, ME)
}

fn install_on() -> Value {
    let mut status = status();
    if let Some(members) = status["members"].as_array_mut() {
        for member in members.iter_mut().filter(|m| m["id"] == STANDBY) {
            member["desired"]["install"] = json!(true);
        }
    }
    status
}

pub fn model_of(which: Shown) -> ClusterModel {
    match which {
        Shown::Empty => model(false),
        Shown::Many => ClusterModel::from_status(&many(), ME),
        Shown::Off => ClusterModel::from_status(&off(), ME),
        Shown::Joining => ClusterModel::from_status(&joining(true), ME),
        Shown::Install => ClusterModel::from_status(&install_on(), ME),
        Shown::Full | Shown::Menu | Shown::InstallOff => model(true),
    }
}

pub fn install_plan_reply(on: bool) -> Value {
    json!({
        "on": on,
        "applied": false,
        "message": "install is off",
        "plan": {
            "action": "install",
            "changed": true,
            "blocked": null,
            "hooks": {
                "file": "/home/maintainer/.claude/settings.json",
                "state": "present",
                "read_only": false,
                "refused": null,
                "changed": true,
                "changes": [
                    "add PreToolUse (AskUserQuestion): '/usr/local/bin/notchling' hook (timeout 300 s)",
                    "add PermissionRequest (*): '/usr/local/bin/notchling' hook (timeout 300 s)",
                    "add PreToolUse (*): '/usr/local/bin/notchling' hook --fence (timeout 30 s)",
                ],
                "notes": [],
                "present": [],
            },
            "skills": [
                {"name": "notchling-delegate", "tool": "claude", "path": "/x", "action": "write", "reason": null, "error": null},
                {"name": "notchling-pages", "tool": "claude", "path": "/x", "action": "write", "reason": null, "error": null},
            ],
        },
    })
}
