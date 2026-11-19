use serde_json::{json, Value};

use notchling_engine::vault::entry::VaultEntry;

use super::feed::ClusterFeed;
use super::model::ClusterModel;
use super::{HEIGHT, WIDTH};
use crate::views::ViewSpec;

pub const ME: &str = "a1b2c3d4e5f60718";
pub const STANDBY: &str = "1122334455667788";
pub const WORKER: &str = "99aabbccddeeff00";
pub const STUDIO: &str = "5566778899aabbcc";
pub const THIS_MAC: &str = "MacBook";
pub const STANDBY_NAME: &str = "mac-mini";

pub const VIEWS: [ViewSpec; 29] = [
    ViewSpec {
        name: "cluster",
        size: (WIDTH, HEIGHT),
        about: "the cluster sheet: this Mac as Primary, a Standby online, a Worker offline with a version warning (made-up data)",
    },
    ViewSpec {
        name: "machines-sheet",
        size: (WIDTH, 760.0),
        about: "the Machines sheet: the Cluster section, two members, two machines that aren't in the cluster with Check, Join Cluster and a menu, and Add Machine (made-up hosts)",
    },
    ViewSpec {
        name: "machines-sheet-add",
        size: (WIDTH, 760.0),
        about: "the Add Machine form over the Machines sheet: user@host typed, the name filled from the host (made-up hosts)",
    },
    ViewSpec {
        name: "machines-sheet-addresses",
        size: (WIDTH, 760.0),
        about: "the Add Machine form with an Addresses list: a Tailscale name on a network, a LAN address, a public address with a port and a jump through another machine (made-up hosts)",
    },
    ViewSpec {
        name: "machines-sheet-tailscale",
        size: (WIDTH, 760.0),
        about: "Add from Tailscale: the machines on the active network with a filter, two chosen with their SSH users, one already in Machines hidden, and the note about the other network (made-up devices)",
    },
    ViewSpec {
        name: "machines-sheet-tailscale-test",
        size: (WIDTH, 1000.0),
        about: "Add from Tailscale with SSH Keys and test results: connected, wrong user or key, a public key warning with a new host, a missing key file and a test still running (made-up devices)",
    },
    ViewSpec {
        name: "machines-sheet-tailscale-trust",
        size: (WIDTH, 1120.0),
        about: "Add from Tailscale after Trust Host on a new host: the host key's fingerprint, the check-it warning and Add to Known Hosts, with a jump-host and a changed-key result on other rows (made-up devices)",
    },
    ViewSpec {
        name: "machines-sheet-edit",
        size: (WIDTH, 760.0),
        about: "a machine's Edit Addresses sheet with its SSH Key, Choose and Clear, and a Test with one line per address: connected, wrong network and can't reach (made-up hosts)",
    },
    ViewSpec {
        name: "machines-sheet-organization",
        size: (WIDTH, 760.0),
        about: "Add from Tailscale with the Organization picker: the imported machines will be filed under Acme instead of Personal (made-up devices and Organizations)",
    },
    ViewSpec {
        name: "machines-sheet-network",
        size: (WIDTH, 760.0),
        about: "the Machines sheet with a machine whose address is on another Tailscale network: the line that says so and Switch Network, beside a machine reached over Tailscale (made-up hosts)",
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
        name: "cluster-replication",
        size: (WIDTH, HEIGHT),
        about: "the cluster sheet with three Can Be Master machines: one with a recent set, one behind, one not ready (made-up data)",
    },
    ViewSpec {
        name: "cluster-move",
        size: (WIDTH, 720.0),
        about: "the Move sheet over the cluster sheet while the primary is moving to the Standby: sessions that move and stay, the steps, Move Now (made-up data)",
    },
    ViewSpec {
        name: "cluster-promote",
        size: (WIDTH, 1000.0),
        about: "the cluster sheet while the primary is silent, on its Standby: Move to Now asking to confirm a risk, a side copy and a waiting session, with no failover line though the status says 30m (made-up data)",
    },
    ViewSpec {
        name: "cluster-take-back",
        size: (WIDTH, HEIGHT),
        about: "the cluster sheet on the old primary after another machine took over, asking to Take Back the primary (made-up data)",
    },
    ViewSpec {
        name: "cluster-move-unfenced",
        size: (WIDTH, 720.0),
        about: "the Move sheet after a move whose old primary couldn't pause the moved sessions: the warning with its reason (made-up data)",
    },
    ViewSpec {
        name: "cluster-joining",
        size: (WIDTH, 300.0),
        about: "the cluster sheet while this Mac waits for the Primary to approve its join (made-up data)",
    },
    ViewSpec {
        name: "cluster-add",
        size: (WIDTH, HEIGHT),
        about: "the Add Machine sheet over the cluster sheet: SSH Address typed, the name filled from the host (made-up data)",
    },
    ViewSpec {
        name: "cluster-add-plan",
        size: (WIDTH, 1200.0),
        about: "the Add Machine sheet at its plan: four steps and a how-to the join doesn't need, the admin password field, Can Be Master off with its reason, Acting on, a how-to to do after it joins, Install and Join (made-up data)",
    },
    ViewSpec {
        name: "cluster-add-running",
        size: (WIDTH, 760.0),
        about: "the Add Machine sheet while a run goes on: Adding m2 (step 3 of 4), the masked log and the Close button (made-up data)",
    },
    ViewSpec {
        name: "cluster-add-stopped",
        size: (WIDTH, 760.0),
        about: "the Add Machine sheet after a run stopped: the reason, its how-to, Check Again (made-up data)",
    },
    ViewSpec {
        name: "cluster-not-joined",
        size: (WIDTH, 820.0),
        about: "the cluster sheet with the Not in the Cluster section: m1 and vps-sing-1 from Machines with Join Cluster (fixture registry)",
    },
    ViewSpec {
        name: "cluster-update",
        size: (WIDTH, 720.0),
        about: "the cluster sheet offering Update 3 Machines, with its sheet open: one machine ready, one waiting for the Linux build, one busy with Update Anyway (made-up data)",
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
    Off,
    Joining,
    Install,
    InstallOff,
    Replication,
    Move,
    Secrets,
    Promote,
    Unfenced,
    TakeBack,
    Add,
    AddPlan,
    AddRunning,
    AddStopped,
    NotJoined,
    Update,
    Machines,
    MachinesAdd,
    MachinesAddresses,
    MachinesTailscale,
    MachinesTest,
    MachinesTrust,
    MachinesEdit,
    MachinesOrganization,
    MachinesNetwork,
}

pub fn shown(name: &str) -> Option<Shown> {
    match name {
        "cluster" => Some(Shown::Full),
        "machines-sheet" => Some(Shown::Machines),
        "machines-sheet-add" => Some(Shown::MachinesAdd),
        "machines-sheet-addresses" => Some(Shown::MachinesAddresses),
        "machines-sheet-tailscale" => Some(Shown::MachinesTailscale),
        "machines-sheet-tailscale-test" => Some(Shown::MachinesTest),
        "machines-sheet-tailscale-trust" => Some(Shown::MachinesTrust),
        "machines-sheet-edit" => Some(Shown::MachinesEdit),
        "machines-sheet-organization" => Some(Shown::MachinesOrganization),
        "machines-sheet-network" => Some(Shown::MachinesNetwork),
        "cluster-empty" => Some(Shown::Empty),
        "cluster-many" => Some(Shown::Many),
        "cluster-menu" => Some(Shown::Menu),
        "cluster-off" => Some(Shown::Off),
        "cluster-joining" => Some(Shown::Joining),
        "cluster-replication" => Some(Shown::Replication),
        "cluster-move" => Some(Shown::Move),
        "cluster-promote" => Some(Shown::Promote),
        "cluster-take-back" => Some(Shown::TakeBack),
        "cluster-move-unfenced" => Some(Shown::Unfenced),
        "cluster-install" => Some(Shown::Install),
        "cluster-install-off" => Some(Shown::InstallOff),
        "cluster-secrets" => Some(Shown::Secrets),
        "cluster-add" => Some(Shown::Add),
        "cluster-add-plan" => Some(Shown::AddPlan),
        "cluster-add-running" => Some(Shown::AddRunning),
        "cluster-add-stopped" => Some(Shown::AddStopped),
        "cluster-not-joined" => Some(Shown::NotJoined),
        "cluster-update" => Some(Shown::Update),
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
        "headless": role == "standby",
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

fn ready_ok() -> Value {
    json!({
        "ok": true,
        "reason": null,
        "checks": [
            {"id": "encrypted", "ok": true, "text": "disk encryption is on"},
            {"id": "home", "ok": true, "text": "maintainer at /Users/maintainer"},
            {"id": "space", "ok": true, "text": "212.0 GB free"},
            {"id": "tools", "ok": true, "text": "Claude Code and Codex match"},
        ],
    })
}

fn ready_failing(reason: &str) -> Value {
    json!({
        "ok": false,
        "reason": reason,
        "checks": [
            {"id": "encrypted", "ok": false, "text": reason},
            {"id": "home", "ok": true, "text": "maintainer at /Users/maintainer"},
            {"id": "space", "ok": true, "text": "180.0 GB free"},
            {"id": "tools", "ok": true, "text": "Claude Code and Codex match"},
        ],
    })
}

pub fn status() -> Value {
    let mut status = base_status();
    set_member(&mut status, STANDBY, "ready", ready_ok());
    status["replication"] = json!([
        {"node": STANDBY, "setAgeS": 130, "sizeMb": 14, "workedAgeS": 40},
    ]);
    status
}

fn set_member(status: &mut Value, id: &str, key: &str, value: Value) {
    if let Some(members) = status["members"].as_array_mut() {
        for member in members.iter_mut().filter(|m| m["id"] == id) {
            member[key] = value.clone();
        }
    }
}

pub fn replication() -> Value {
    let mut status = status();
    let mut studio = member(
        STUDIO,
        "mac-studio",
        "standby",
        true,
        true,
        versions("macos", "aarch64", Some("2.1.320"), "0.170.0"),
        json!({"cpu": 4, "memUsedMb": 20480, "memTotalMb": 65536}),
        false,
    );
    studio["ready"] = ready_failing("mac-studio has no disk encryption");
    if let Some(members) = status["members"].as_array_mut() {
        members.push(studio);
    }
    status["replication"] = json!([
        {"node": STANDBY, "setAgeS": 1260, "sizeMb": 9, "workedAgeS": 300},
        {"node": STUDIO, "setAgeS": null, "sizeMb": null, "workedAgeS": 300},
    ]);
    status
}

pub fn moving(step: &str) -> Value {
    let mut status = status();
    status["move"] = json!({
        "to": STANDBY,
        "step": step,
        "moving": ["j-parser", "codex:c-docs", "j-tests"],
        "staying": [
            ["j-bench", "Its folder has changes that aren't pushed"],
            ["j-fork", "Its branch isn't pushed yet"],
        ],
        "cut": if step == "freeze" { json!(["j-tests"]) } else { json!([]) },
        "stopped": [],
        "failed": [],
        "reason": null,
    });
    status
}

pub const RENDER_NOW: i64 = 1_800_000_000_000;

pub fn silent() -> Value {
    let mut status = status();
    set_member(&mut status, ME, "online", json!(false));
    status["me"] = json!(STANDBY);
    status
}

pub fn stepped_down() -> Value {
    let mut status = status();
    status["cluster"]["home"] = json!(STANDBY);
    set_member(&mut status, ME, "role", json!("standby"));
    set_member(&mut status, STANDBY, "role", json!("home"));
    status
}

pub fn with_failover(
    mut status: Value,
    value: &str,
    armed: bool,
    takes_over_at: Option<i64>,
) -> Value {
    status["failover"] = json!({
        "value": value,
        "armed": armed,
        "target": STANDBY,
        "takesOverAt": takes_over_at,
    });
    status
}

pub fn side_copy(session: &str, node: &str, path: Option<&str>, at_ms: i64, hidden: bool) -> Value {
    json!({
        "session": session,
        "node": node,
        "base_set": "set-7",
        "extra_bytes": 48_000,
        "repo_changed": false,
        "path": path,
        "at_ms": at_ms,
        "hidden": hidden,
    })
}

pub fn waiting_entry(session: &str, node: &str, name: &str, title: Option<&str>) -> Value {
    json!({"session": session, "node": node, "name": name, "title": title})
}

pub fn claim(session: &str, node: &str) -> Value {
    json!({
        "session": session,
        "node": node,
        "n": 2,
        "term": {"n": 2, "node": node},
        "base_set": "set-7",
        "base_len": 1200,
        "touched": false,
    })
}

fn render_promote() -> Value {
    let mut status = with_failover(silent(), "30m", false, None);
    status["sideCopies"] = json!([
        side_copy(
            "j-bench",
            ME,
            Some("/Users/maintainer/.claude/projects/p/bench.jsonl"),
            RENDER_NOW - 37 * 60_000,
            false,
        ),
        side_copy(
            "j-fork",
            STANDBY,
            Some("/Users/maintainer/.claude/projects/p/fork.jsonl"),
            RENDER_NOW - 5 * 3_600_000,
            false,
        ),
        side_copy("j-old", ME, None, RENDER_NOW - 90_000_000, true),
    ]);
    status["waiting"] = json!([waiting_entry("j-tests", ME, "MacBook", None)]);
    status
}

pub fn back() -> Value {
    let mut status = stepped_down();
    status["sideCopies"] = json!([side_copy(
        "j-bench",
        ME,
        Some("/Users/maintainer/.claude/projects/p/bench.jsonl"),
        RENDER_NOW - 12 * 60_000,
        false,
    )]);
    status
}

pub fn risk_reply() -> Value {
    json!({
        "needsConfirm": "MacBook may still be running its sessions. If it is, they keep going until it hears of this."
    })
}

pub fn unfenced(step: &str) -> Value {
    let mut status = stepped_down();
    status["move"] = moving(step)["move"].clone();
    status["move"]["unfenced"] = json!("MacBook didn't answer the pause request");
    status
}

pub fn keep_model(session: &str) -> ClusterModel {
    let mut status = status();
    status["main"] = json!([session]);
    ClusterModel::from_status(&status, ME)
}

pub fn names() -> std::collections::BTreeMap<String, String> {
    [
        ("j-parser", "Fix the parser"),
        ("codex:c-docs", "Write the docs"),
        ("j-tests", "Run the full test suite"),
        ("j-bench", "Benchmark the sync"),
        ("j-fork", "Try the fork"),
    ]
    .into_iter()
    .map(|(id, name)| (id.to_owned(), name.to_owned()))
    .collect()
}

fn base_status() -> Value {
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
        "main": ["j-parser", "codex:c-docs", "j-tests"],
        "kept": ["j-bench"],
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

pub fn updating() -> Value {
    let mut status = status();
    set_member(&mut status, STANDBY, "name", json!("m1"));
    set_member(&mut status, WORKER, "name", json!("vps-sing-1"));
    let mut third = member(
        STUDIO,
        "m3",
        "worker",
        false,
        true,
        versions("macos", "aarch64", Some("2.1.320"), "0.170.0"),
        json!({"cpu": 4, "memUsedMb": 20480, "memTotalMb": 65536}),
        false,
    );
    third["ready"] = Value::Null;
    if let Some(members) = status["members"].as_array_mut() {
        members.push(third);
    }
    status
}

pub fn two_members() -> Value {
    let mut status = status();
    if let Some(members) = status["members"].as_array_mut() {
        members.retain(|m| m["id"] != WORKER);
    }
    status
}

pub fn model_of(which: Shown) -> ClusterModel {
    match which {
        Shown::Machines
        | Shown::MachinesAdd
        | Shown::MachinesAddresses
        | Shown::MachinesTailscale
        | Shown::MachinesTest
        | Shown::MachinesTrust
        | Shown::MachinesEdit
        | Shown::MachinesOrganization
        | Shown::MachinesNetwork => ClusterModel::from_status(&two_members(), ME),
        Shown::Update => ClusterModel::from_status(&updating(), ME),
        Shown::Empty => model(false),
        Shown::Many => ClusterModel::from_status(&many(), ME),
        Shown::Off => ClusterModel::from_status(&off(), ME),
        Shown::Joining => ClusterModel::from_status(&joining(true), ME),
        Shown::Install => ClusterModel::from_status(&install_on(), ME),
        Shown::Replication => ClusterModel::from_status(&replication(), ME),
        Shown::Move => ClusterModel::from_status(&moving("freeze"), ME),
        Shown::Promote => ClusterModel::from_status(&render_promote(), STANDBY),
        Shown::Unfenced => ClusterModel::from_status(&unfenced("done"), ME),
        Shown::TakeBack => ClusterModel::from_status(&back(), ME),
        Shown::Full
        | Shown::Menu
        | Shown::InstallOff
        | Shown::Secrets
        | Shown::Add
        | Shown::AddPlan
        | Shown::AddRunning
        | Shown::AddStopped
        | Shown::NotJoined => model(true),
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
