use serde_json::{json, Value};

use super::model::ClusterModel;
use super::{HEIGHT, WIDTH};
use crate::views::ViewSpec;

pub const ME: &str = "a1b2c3d4e5f60718";
pub const STANDBY: &str = "1122334455667788";
pub const WORKER: &str = "99aabbccddeeff00";

pub const VIEWS: [ViewSpec; 2] = [
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
];

pub fn shown(name: &str) -> Option<bool> {
    match name {
        "cluster" => Some(true),
        "cluster-empty" => Some(false),
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
        "headless": false,
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

pub fn model(full: bool) -> ClusterModel {
    ClusterModel::from_status(&if full { status() } else { empty() }, ME)
}
