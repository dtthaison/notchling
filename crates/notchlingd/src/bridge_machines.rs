use notchling_engine::bridge::model::ClusterMachine;
use serde_json::Value;

fn ready(member: &Value) -> bool {
    member["ready"]["ok"].as_bool() != Some(false)
}

fn set_at(
    status: &Value,
    id: &str,
    me: Option<&str>,
    own: Option<i64>,
    now_ms: i64,
) -> Option<i64> {
    if me == Some(id) {
        return own;
    }
    status["replication"]
        .as_array()?
        .iter()
        .find(|entry| entry["node"].as_str() == Some(id))
        .and_then(|entry| entry["setAgeS"].as_u64())
        .map(|age| now_ms.saturating_sub((age as i64).saturating_mul(1000)))
}

pub fn machines(status: &Value, own_set_ms: Option<i64>, now_ms: i64) -> Vec<ClusterMachine> {
    let me = status["me"].as_str();
    status["members"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["state"].as_str().is_none_or(|state| state == "active"))
        .filter_map(|m| {
            let id = m["id"].as_str()?;
            let can_be_master = m["canBeMaster"].as_bool() == Some(true);
            let ready = ready(m);
            let set_at = set_at(status, id, me, own_set_ms, now_ms);
            Some(ClusterMachine {
                id: id.to_string(),
                name: m["name"].as_str().unwrap_or(id).to_string(),
                online: m["online"].as_bool() == Some(true),
                can_be_master,
                ready,
                set_at,
                can_resume: can_be_master && ready && set_at.is_some(),
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "bridge_machines_tests.rs"]
mod tests;
