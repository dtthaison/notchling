use notchling_locale as t;
use serde_json::Value;

pub const BEHIND_S: u64 = 900;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplicationRow {
    pub node: String,
    pub last_set_age_s: Option<u64>,
    pub size_mb: Option<u64>,
    pub behind: bool,
    pub ready: Result<(), String>,
    pub reported: bool,
}

fn seconds(entry: &Value, key: &str) -> Option<u64> {
    entry.get(key).and_then(Value::as_u64)
}

fn members(status: &Value) -> Vec<&Value> {
    status
        .get("members")
        .and_then(Value::as_array)
        .map(|m| m.iter().collect())
        .unwrap_or_default()
}

fn ready_of(member: &Value) -> Result<(), String> {
    let Some(ready) = member.get("ready").filter(|r| r.is_object()) else {
        return Ok(());
    };
    if ready.get("ok").and_then(Value::as_bool) != Some(false) {
        return Ok(());
    }
    let reason = ready
        .get("reason")
        .and_then(Value::as_str)
        .filter(|r| !r.is_empty())
        .map(str::to_owned);
    let failing = || {
        ready
            .get("checks")
            .and_then(Value::as_array)?
            .iter()
            .find(|c| c.get("ok").and_then(Value::as_bool) == Some(false))
            .and_then(|c| c.get("text").and_then(Value::as_str))
            .map(str::to_owned)
    };
    Err(reason
        .or_else(failing)
        .unwrap_or_else(|| t::cluster::not_ready_unknown().to_owned()))
}

pub fn is_behind(set_age_s: Option<u64>, worked_age_s: Option<u64>) -> bool {
    let Some(worked) = worked_age_s else {
        return false;
    };
    match set_age_s {
        None => true,
        Some(set) => set >= BEHIND_S && worked < set,
    }
}

impl ReplicationRow {
    pub fn from(status: &Value, node: &str) -> ReplicationRow {
        let member = members(status)
            .into_iter()
            .find(|m| m.get("id").and_then(Value::as_str) == Some(node));
        let entry = status
            .get("replication")
            .and_then(Value::as_array)
            .and_then(|all| {
                all.iter()
                    .find(|e| e.get("node").and_then(Value::as_str) == Some(node))
            });
        let age = entry.and_then(|e| seconds(e, "setAgeS"));
        let worked = entry.and_then(|e| seconds(e, "workedAgeS"));
        ReplicationRow {
            node: node.to_owned(),
            last_set_age_s: age,
            size_mb: entry.and_then(|e| seconds(e, "sizeMb")),
            behind: is_behind(age, worked),
            ready: member.map_or(Ok(()), ready_of),
            reported: entry.is_some(),
        }
    }

    pub fn offered(member: &Value) -> bool {
        member.get("canBeMaster").and_then(Value::as_bool) == Some(true)
            && member.get("state").and_then(Value::as_str) == Some("active")
            && member.get("role").and_then(Value::as_str) != Some("home")
    }

    pub fn rows(status: &Value) -> Vec<ReplicationRow> {
        members(status)
            .into_iter()
            .filter(|m| ReplicationRow::offered(m))
            .filter_map(|m| m.get("id").and_then(Value::as_str))
            .map(|id| ReplicationRow::from(status, id))
            .collect()
    }

    pub fn not_ready_text(&self) -> Option<String> {
        self.ready
            .as_ref()
            .err()
            .map(|reason| t::cluster::not_ready(reason))
    }

    pub fn set_text(&self) -> Option<String> {
        if self.ready.is_err() || !self.reported {
            return None;
        }
        let Some(age) = self.last_set_age_s else {
            return Some(t::cluster::set_none().to_owned());
        };
        Some(match self.size_mb {
            Some(mb) => t::cluster::set_last_sized(&ago(age), mb),
            None => t::cluster::set_last(&ago(age)),
        })
    }

    pub fn behind_text(&self) -> Option<String> {
        if !self.behind || self.ready.is_err() {
            return None;
        }
        Some(match self.last_set_age_s {
            Some(age) => t::cluster::set_behind(&span(age)),
            None => t::cluster::set_behind_none().to_owned(),
        })
    }
}

pub fn ago(seconds: u64) -> String {
    let seconds = i64::try_from(seconds).unwrap_or(i64::MAX);
    if seconds < 60 {
        t::units::just_now().to_owned()
    } else if seconds < 3600 {
        t::units::minutes_ago(seconds / 60)
    } else {
        t::units::hours_ago(seconds / 3600)
    }
}

fn span(seconds: u64) -> String {
    let seconds = i64::try_from(seconds).unwrap_or(i64::MAX);
    if seconds < 3600 {
        t::units::age_minutes((seconds / 60).max(1))
    } else {
        t::units::age_hours(seconds / 3600)
    }
}
