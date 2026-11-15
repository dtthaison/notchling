use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::id::NodeId;
use super::members::Member;
use super::versions::Versions;
use crate::versions::Tool;

pub const MIN_FREE_MB: u64 = 5 * 1024;
pub const SET_SPACE_FACTOR: u64 = 2;
const MATCHED_TOOLS: [Tool; 2] = [Tool::Claude, Tool::Codex];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct HostFacts {
    pub encrypted: Option<bool>,
    pub home: String,
    pub user: String,
    pub free_mb: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckKind {
    Encrypted,
    SameHome,
    Space,
    Tools,
}

impl CheckKind {
    pub const ALL: [CheckKind; 4] = [
        CheckKind::Encrypted,
        CheckKind::SameHome,
        CheckKind::Space,
        CheckKind::Tools,
    ];

    pub fn key(self) -> &'static str {
        match self {
            CheckKind::Encrypted => "encrypted",
            CheckKind::SameHome => "home",
            CheckKind::Space => "space",
            CheckKind::Tools => "tools",
        }
    }

    pub fn from_key(key: &str) -> Option<CheckKind> {
        CheckKind::ALL.into_iter().find(|k| k.key() == key)
    }
}

#[derive(Clone, Copy)]
pub struct Subject<'a> {
    pub name: &'a str,
    pub versions: Option<&'a Versions>,
}

impl<'a> Subject<'a> {
    pub fn of(member: &'a Member) -> Subject<'a> {
        Subject {
            name: &member.name,
            versions: member.versions.as_ref(),
        }
    }
}

fn gb(mb: u64) -> String {
    format!("{:.1} GB", mb as f64 / 1024.0)
}

fn host_of<'a>(s: Subject<'a>) -> Result<&'a HostFacts, String> {
    s.versions
        .and_then(|v| v.host.as_ref())
        .ok_or_else(|| format!("{} hasn't reported its disk yet", s.name))
}

pub fn check(
    kind: CheckKind,
    member: Subject,
    primary: Subject,
    last_set_mb: u64,
) -> Result<String, String> {
    match kind {
        CheckKind::Encrypted => encrypted(member),
        CheckKind::SameHome => same_home(member, primary),
        CheckKind::Space => space(member, last_set_mb),
        CheckKind::Tools => tools(member, primary),
    }
}

fn encrypted(member: Subject) -> Result<String, String> {
    match host_of(member)?.encrypted {
        Some(true) => Ok("disk encryption is on".to_string()),
        Some(false) => Err(format!("{} has no disk encryption", member.name)),
        None => Err(format!(
            "{} can't tell whether its disk encryption is on",
            member.name
        )),
    }
}

fn same_home(member: Subject, primary: Subject) -> Result<String, String> {
    let mine = host_of(member)?;
    let theirs = host_of(primary)?;
    if mine.home != theirs.home || mine.user != theirs.user {
        return Err(format!(
            "{} is {} at {}, {} is {} at {}",
            member.name, mine.user, mine.home, primary.name, theirs.user, theirs.home
        ));
    }
    Ok(format!("{} at {}", mine.user, mine.home))
}

fn space(member: Subject, last_set_mb: u64) -> Result<String, String> {
    let free = host_of(member)?.free_mb;
    let need = last_set_mb
        .saturating_mul(SET_SPACE_FACTOR)
        .max(MIN_FREE_MB);
    if free < need {
        return Err(format!(
            "{} has {} free, needs {}",
            member.name,
            gb(free),
            gb(need)
        ));
    }
    Ok(format!("{} free", gb(free)))
}

fn tools(member: Subject, primary: Subject) -> Result<String, String> {
    let Some(want) = primary.versions else {
        return Err(format!("{} hasn't reported its tools yet", primary.name));
    };
    let Some(have) = member.versions else {
        return Err(format!("{} hasn't reported its tools yet", member.name));
    };
    let mut same = Vec::new();
    for tool in MATCHED_TOOLS {
        let Some(wanted) = want.tools.get(&tool) else {
            continue;
        };
        match have.tools.get(&tool) {
            None => {
                return Err(format!(
                    "{} has no {}, {} runs {wanted}",
                    member.name,
                    tool.title(),
                    primary.name
                ))
            }
            Some(got) if got != wanted => {
                return Err(format!(
                    "{} runs {} {got}, {} {wanted}",
                    member.name,
                    tool.title(),
                    primary.name
                ))
            }
            Some(got) => same.push(format!("{} {got}", tool.title())),
        }
    }
    if same.is_empty() {
        return Err(format!("{} hasn't reported its tools yet", primary.name));
    }
    Ok(same.join(", "))
}

pub fn checks(member: Subject, primary: Subject, last_set_mb: u64) -> Checks {
    CheckKind::ALL
        .into_iter()
        .map(|kind| (kind, check(kind, member, primary, last_set_mb)))
        .collect()
}

pub fn first_failure(checks: &[(CheckKind, Result<String, String>)]) -> Result<(), String> {
    for (_, result) in checks {
        if let Err(why) = result {
            return Err(why.clone());
        }
    }
    Ok(())
}

pub fn ready(member: &Member, primary: &Member, last_set_mb: u64) -> Result<(), String> {
    if !member.can_be_master {
        return Err(format!("{} isn't set to Can Be Master", member.name));
    }
    first_failure(&checks(
        Subject::of(member),
        Subject::of(primary),
        last_set_mb,
    ))
}

pub type Checks = Vec<(CheckKind, Result<String, String>)>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assessment {
    pub checks: Checks,
    pub ready: Result<(), String>,
}

fn versions_in(member: &Value) -> Option<Versions> {
    serde_json::from_value(member.get("versions")?.clone()).ok()
}

pub fn assess(status: &Value, last_set_mb: u64) -> BTreeMap<NodeId, Assessment> {
    let mut out = BTreeMap::new();
    let Some(members) = status.get("members").and_then(Value::as_array) else {
        return out;
    };
    let home = status
        .get("cluster")
        .and_then(|c| c.get("home"))
        .and_then(Value::as_str);
    let primary = members
        .iter()
        .find(|m| home.is_some() && m.get("id").and_then(Value::as_str) == home);
    let primary_name = primary
        .and_then(|m| m.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("the primary");
    let primary_versions = primary.and_then(versions_in);
    for m in members {
        let Some(id) = m.get("id").and_then(Value::as_str).and_then(NodeId::parse) else {
            continue;
        };
        if m.get("canBeMaster").and_then(Value::as_bool) != Some(true)
            || m.get("state").and_then(Value::as_str) != Some("active")
        {
            continue;
        }
        let name = m
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("this machine");
        let versions = versions_in(m);
        let subject = Subject {
            name,
            versions: versions.as_ref(),
        };
        let against = Subject {
            name: primary_name,
            versions: primary_versions.as_ref(),
        };
        let list = if primary.is_some() {
            checks(subject, against, last_set_mb)
        } else {
            CheckKind::ALL
                .into_iter()
                .map(|k| (k, Err("the cluster has no primary to match".to_string())))
                .collect()
        };
        let ready = first_failure(&list);
        out.insert(
            id,
            Assessment {
                checks: list,
                ready,
            },
        );
    }
    out
}

pub fn annotate(status: &mut Value, last_set_mb: u64) {
    let found = assess(status, last_set_mb);
    let Some(members) = status.get_mut("members").and_then(Value::as_array_mut) else {
        return;
    };
    for m in members {
        let Some(a) = m
            .get("id")
            .and_then(Value::as_str)
            .and_then(NodeId::parse)
            .and_then(|id| found.get(&id))
        else {
            continue;
        };
        let list: Vec<Value> = a
            .checks
            .iter()
            .map(|(kind, result)| {
                json!({
                    "id": kind.key(),
                    "ok": result.is_ok(),
                    "text": match result { Ok(t) | Err(t) => t },
                })
            })
            .collect();
        m["ready"] = json!({
            "ok": a.ready.is_ok(),
            "reason": a.ready.as_ref().err(),
            "checks": list,
        });
    }
}

pub fn ready_in(status: &Value, node: &NodeId, last_set_mb: u64) -> bool {
    assess(status, last_set_mb)
        .get(node)
        .is_some_and(|a| a.ready.is_ok())
}

pub fn checks_in(status: &Value, node: &str) -> Option<Checks> {
    let member = status
        .get("members")?
        .as_array()?
        .iter()
        .find(|m| m.get("id").and_then(Value::as_str) == Some(node))?;
    if member.get("canBeMaster").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let list = member.get("ready")?.get("checks")?.as_array()?;
    let mut out = Vec::new();
    for entry in list {
        let kind = CheckKind::from_key(entry.get("id")?.as_str()?)?;
        let text = entry.get("text")?.as_str()?.to_string();
        out.push((
            kind,
            if entry.get("ok")?.as_bool()? {
                Ok(text)
            } else {
                Err(text)
            },
        ));
    }
    Some(out)
}
