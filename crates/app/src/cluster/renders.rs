use serde_json::{json, Value};

use notchling_engine::vault::entry::VaultEntry;

use super::feed::ClusterFeed;
use super::model::ClusterModel;
use super::{HEIGHT, WIDTH};
use crate::views::ViewSpec;

pub const ME: &str = "a1b2c3d4e5f60718";
pub const STANDBY: &str = "1122334455667788";
pub const WORKER: &str = "99aabbccddeeff00";

pub const VIEWS: [ViewSpec; 5] = [
    ViewSpec {
        name: "cluster",
        size: (WIDTH, HEIGHT),
        about: "the cluster sheet: this Mac as Primary, a Standby online, a Worker offline with a version warning (made-up data)",
    },
    ViewSpec {
        name: "cluster-empty",
        size: (WIDTH, 300.0),
        about: "the cluster sheet before a cluster exists: Create Cluster (made-up data)",
    },
    ViewSpec {
        name: "cluster-many",
        size: (WIDTH, HEIGHT),
        about: "the cluster sheet with more machines than fit, so it scrolls (made-up data)",
    },
    ViewSpec {
        name: "cluster-menu",
        size: (WIDTH, HEIGHT),
        about: "the cluster sheet with the Standby row's menu open (made-up data)",
    },
    ViewSpec {
        name: "cluster-secrets",
        size: (WIDTH, HEIGHT),
        about: "the Worker's Secrets editor: the vault's names, two allowed, Any Secret off (made-up data)",
    },
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shown {
    Full,
    Empty,
    Many,
    Menu,
    Secrets,
}

pub fn shown(name: &str) -> Option<Shown> {
    match name {
        "cluster" => Some(Shown::Full),
        "cluster-empty" => Some(Shown::Empty),
        "cluster-many" => Some(Shown::Many),
        "cluster-menu" => Some(Shown::Menu),
        "cluster-secrets" => Some(Shown::Secrets),
        _ => None,
    }
}

fn versions(os: &str, arch: &str, claude: Option<&str>, codex: &str) -> Value {
    versions_with(os, arch, claude, codex, json!({}))
}

fn versions_with(os: &str, arch: &str, claude: Option<&str>, codex: &str, logins: Value) -> Value {
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
        "logins": logins,
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
        "headless": false,
        "reference": reference,
        "state": "active",
        "versions": versions,
        "load": load,
        "desired": {"acting": role == "home", "bridge": role == "home", "install": false},
        "acceptsDelegation": role != "home",
        "secrets": {"names": [], "any": false},
    })
}

pub fn allow(status: &mut Value, id: &str, names: &[&str], any: bool) {
    let member = status["members"]
        .as_array_mut()
        .and_then(|members| members.iter_mut().find(|m| m["id"] == id));
    if let Some(member) = member {
        member["secrets"] = json!({"names": names, "any": any});
    }
}

pub fn vault_names() -> Vec<String> {
    ["github", "npm", "registry", "weather-api"]
        .map(str::to_owned)
        .to_vec()
}

pub fn status() -> Value {
    let mut status = json!({
        "cluster": {"id": "c-1", "name": "Studio", "term": {"n": 1, "node": ME}, "home": ME, "vault": ME},
        "me": ME,
        "members": [
            member(
                WORKER,
                "build-box",
                "worker",
                false,
                false,
                versions_with("linux", "x86_64", None, "0.166.0", json!({"antigravity": false})),
                Value::Null,
                false,
            ),
            member(
                STANDBY,
                "mac-mini",
                "standby",
                true,
                true,
                versions_with(
                    "macos",
                    "aarch64",
                    Some("2.1.320"),
                    "0.170.0",
                    json!({"codex": true, "antigravity": true}),
                ),
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
    });
    allow(&mut status, WORKER, &["github", "registry"], false);
    status
}

pub fn feed_with_remote_vault(entries: Vec<VaultEntry>) -> ClusterFeed {
    let mut moved = status();
    moved["cluster"]["vault"] = json!(STANDBY);
    ClusterFeed::fixed_with_vault(ClusterModel::from_status(&moved, ME), entries)
}

pub fn empty() -> Value {
    json!({"cluster": null, "me": ME, "members": [], "link": false, "listening": null, "tailscale": true, "error": null})
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

pub fn model_of(which: Shown) -> ClusterModel {
    match which {
        Shown::Empty => model(false),
        Shown::Many => ClusterModel::from_status(&many(), ME),
        Shown::Full | Shown::Menu | Shown::Secrets => model(true),
    }
}
