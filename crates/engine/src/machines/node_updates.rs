use super::dist_manifest::DistManifest;
use super::registry::Machine;
use std::collections::BTreeSet;

const BUILD_LINUX: &str = "Build the Linux nodes first: tools/build-node.sh";
const BUILD_MAC: &str = "Build Notchling for the other Mac first: tools/build-node.sh --mac-intel";
const UNKNOWN_SYSTEM: &str = "the machine's system and chip aren't known yet; check it first";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberBuild {
    pub node: String,
    pub name: String,
    pub os: String,
    pub arch: String,
    pub build: Option<String>,
    pub running_shell_jobs: usize,
    pub moving: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateState {
    Ready,
    NeedsBuild(String),
    Busy(String),
    NotInMachines,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateRow {
    pub machine: String,
    pub state: UpdateState,
}

fn arch_word(raw: &str) -> &str {
    if raw == "arm64" {
        "aarch64"
    } else {
        raw
    }
}

fn dist_file(os: &str, arch: &str) -> Option<String> {
    let known = matches!(os, "linux" | "macos") && matches!(arch, "x86_64" | "aarch64");
    known.then(|| format!("notchling-{os}-{arch}"))
}

fn needs_build(
    this_build: Option<&str>,
    member: &MemberBuild,
    manifest: Option<&DistManifest>,
    can_download: bool,
) -> Option<String> {
    let arch = arch_word(&member.arch);
    let Some(file) = dist_file(&member.os, arch) else {
        return Some(UNKNOWN_SYSTEM.into());
    };
    let own = member.os == "macos" && arch == std::env::consts::ARCH;
    if own {
        return None;
    }
    let build = this_build?;
    let listed = manifest
        .filter(|m| m.build == build)
        .is_some_and(|m| m.files.contains_key(&file));
    if listed || (can_download && member.os == "linux") {
        return None;
    }
    Some(
        if member.os == "linux" {
            BUILD_LINUX
        } else {
            BUILD_MAC
        }
        .into(),
    )
}

fn different(
    this_build: Option<&str>,
    member: &MemberBuild,
    machine: &Machine,
    failed_binary: &BTreeSet<String>,
) -> bool {
    match (this_build, member.build.as_deref()) {
        (Some(ours), Some(theirs)) => ours != theirs,
        _ => failed_binary.contains(&machine.name),
    }
}

pub fn offer(
    this_build: Option<&str>,
    members: &[MemberBuild],
    machines: &[Machine],
    manifest: Option<&DistManifest>,
    failed_binary: &BTreeSet<String>,
    can_download: bool,
) -> Vec<UpdateRow> {
    let mut rows = Vec::new();
    for member in members {
        let Some(machine) = machines.iter().find(|m| m.name == member.name) else {
            let differs = matches!(
                (this_build, member.build.as_deref()),
                (Some(ours), Some(theirs)) if ours != theirs
            );
            if differs {
                rows.push(UpdateRow {
                    machine: member.name.clone(),
                    state: UpdateState::NotInMachines,
                });
            }
            continue;
        };
        if !different(this_build, member, machine, failed_binary) {
            continue;
        }
        let state = if let Some(why) = needs_build(this_build, member, manifest, can_download) {
            UpdateState::NeedsBuild(why)
        } else if member.moving {
            UpdateState::Busy("a move to another primary is running".into())
        } else if member.running_shell_jobs > 0 {
            UpdateState::Busy(format!(
                "{} shell job(s) running; the update restarts the daemon",
                member.running_shell_jobs
            ))
        } else {
            UpdateState::Ready
        };
        rows.push(UpdateRow {
            machine: machine.name.clone(),
            state,
        });
    }
    rows
}

pub fn runnable(rows: &[UpdateRow], anyway: &[String]) -> Vec<String> {
    rows.iter()
        .filter(|row| match row.state {
            UpdateState::Ready => true,
            UpdateState::Busy(_) => anyway.contains(&row.machine),
            _ => false,
        })
        .map(|row| row.machine.clone())
        .collect()
}
