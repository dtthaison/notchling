use crate::inbox_ops::InboxOps;
use crate::limits_ops::LimitsOps;
use crate::op_group::OpGroup;
use crate::providers_ops::ProvidersOps;
use notchling_engine::delegate::options::Tool;
use notchling_engine::delegate::quota::{self, Ready, Window};
use notchling_engine::inbox::Item;
use notchling_engine::organizations::{Kind, Organizations, PERSONAL_ID};
use notchling_engine::providers::ProviderId;
use notchling_local_api::{
    bound_identity, ops, AgentIdentity, ApiError, Caller, OpSpec, Tier,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

pub const READ_OPS: [&str; 4] = [
    ops::INBOX_MINE,
    ops::ACCOUNTS_SUMMARY,
    ops::LIMITS_SUMMARY,
    ops::ORGANIZATIONS_MINE,
];

pub type Clock = Arc<dyn Fn() -> f64 + Send + Sync>;

pub struct ReadsOps {
    orgs: Organizations,
    home: PathBuf,
    sessions: Arc<Mutex<Value>>,
    inbox: Arc<InboxOps>,
    providers: Arc<ProvidersOps>,
    limits: Arc<LimitsOps>,
    quota_dir: PathBuf,
    now: Clock,
}

pub fn mask_email(email: &str) -> String {
    let Some((local, domain)) = email.split_once('@') else {
        return String::new();
    };
    let first: String = local.chars().take(1).collect();
    let dom: String = domain.chars().take(1).collect();
    let tld = domain.rsplit_once('.').map_or("", |(_, t)| t);
    format!("{first}***@{dom}***.{tld}")
}

fn no_session(extra: Value) -> Value {
    let mut out = json!({"reason": "no_session"});
    if let (Some(map), Some(more)) = (out.as_object_mut(), extra.as_object()) {
        for (k, v) in more {
            map.insert(k.clone(), v.clone());
        }
    }
    out
}

fn kind_of(provider: &str) -> Option<(Kind, &'static str)> {
    match provider {
        ProviderId::CLAUDE_CODE => Some((Kind::Claude, "claude")),
        ProviderId::CODEX => Some((Kind::Codex, "codex")),
        ProviderId::ANTIGRAVITY => Some((Kind::Antigravity, "agy")),
        _ => None,
    }
}

fn ready_value(r: &Ready) -> Value {
    json!({
        "usable": r.usable,
        "reason": r.reason,
        "fetched": r.fetched,
        "ageSecs": r.age_secs,
        "pools": r.pools.iter().map(|p| json!({
            "name": p.name,
            "usable": p.usable,
            "reason": p.reason,
            "windows": p.windows.iter().map(|w| json!({
                "label": w.label, "percent": w.percent, "resetsAt": w.resets_at,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

impl ReadsOps {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        orgs: Organizations,
        home: &Path,
        sessions: Arc<Mutex<Value>>,
        inbox: Arc<InboxOps>,
        providers: Arc<ProvidersOps>,
        limits: Arc<LimitsOps>,
        quota_dir: PathBuf,
        now: Clock,
    ) -> ReadsOps {
        ReadsOps {
            orgs,
            home: home.to_path_buf(),
            sessions,
            inbox,
            providers,
            limits,
            quota_dir,
            now,
        }
    }

    fn folder_of(&self, session: &str) -> Option<String> {
        let list = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        list.as_array()?
            .iter()
            .find(|row| row.get("id").and_then(Value::as_str) == Some(session))
            .and_then(|row| row.get("cwd").and_then(Value::as_str))
            .filter(|cwd| !cwd.is_empty())
            .map(str::to_string)
    }

    fn org_of_folder(&self, folder: &str) -> String {
        self.orgs
            .view()
            .folder_here(Path::new(folder), &self.home)
            .organization
    }

    fn org_of_session(&self, session: &str) -> Option<String> {
        Some(self.org_of_folder(&self.folder_of(session)?))
    }

    fn mine(&self, caller: &Caller) -> Option<(String, Option<String>)> {
        if caller.tier == Tier::App {
            return None;
        }
        let who: AgentIdentity = bound_identity()?;
        if !who.verified {
            return None;
        }
        Some((who.organization?, who.session))
    }

    fn inbox(&self, caller: &Caller) -> Value {
        let Some((org, session)) = self.mine(caller) else {
            return no_session(json!({"own": [], "organization": [], "otherOrganizations": 0}));
        };
        let (mut own, mut same, mut others) = (Vec::new(), Vec::new(), 0usize);
        for item in self.inbox.own_items() {
            if item.source.session.is_some() && item.source.session == session {
                own.push(own_item(&item));
                continue;
            }
            let theirs = item.source.session.as_deref().and_then(|s| self.org_of_session(s));
            if theirs.as_deref() == Some(org.as_str()) {
                same.push(short_item(&item));
            } else {
                others += 1;
            }
        }
        json!({"own": own, "organization": same, "otherOrganizations": others})
    }

    fn accounts(&self, caller: &Caller) -> Value {
        let Some((org, _)) = self.mine(caller) else {
            return no_session(json!({"tools": {}}));
        };
        let mut tools = serde_json::Map::new();
        for (provider, state) in self.providers.states() {
            let Some((kind, tool)) = kind_of(&provider) else {
                continue;
            };
            let rows: Vec<Value> = state
                .accounts
                .iter()
                .filter(|a| self.orgs.resolve(kind, &a.id).id == org)
                .map(|a| {
                    json!({
                        "id": a.id,
                        "name": mask_email(&a.email),
                        "organization": org,
                        "active": state.active_id.as_deref() == Some(a.id.as_str()),
                        "plan": a.plan,
                        "needsLogin": a.needs_login,
                    })
                })
                .collect();
            tools.insert(tool.to_string(), json!(rows));
        }
        json!({"organization": org, "tools": tools})
    }

    fn active_in(&self, kind: Kind, id: Option<&str>, org: &str) -> bool {
        id.is_some_and(|id| self.orgs.resolve(kind, id).id == org)
    }

    fn limits(&self, caller: &Caller) -> Value {
        let Some((org, _)) = self.mine(caller) else {
            return no_session(json!({"tools": {}}));
        };
        let now = (self.now)();
        let states = self.providers.states();
        let active = |provider: &str| {
            states
                .iter()
                .find(|(p, _)| p == provider)
                .and_then(|(_, s)| s.active_id.clone())
        };
        let mut tools = serde_json::Map::new();
        let claude = self.limits.claude_observed();
        let claude_active = claude.as_ref().and_then(|o| o.active_id.clone());
        let ready = if self.active_in(Kind::Claude, claude_active.as_deref(), &org) {
            let seen = claude.unwrap_or_default();
            let windows = seen
                .windows
                .iter()
                .map(|(label, percent, resets_at)| Window {
                    label: label.clone(),
                    percent: *percent,
                    resets_at: *resets_at,
                })
                .collect();
            Some(quota::readiness_of_windows(seen.checked, true, windows, now))
        } else {
            None
        };
        tools.insert("claude".into(), self.tool_value(ready));
        for (tool, kind, provider) in [
            (Tool::Codex, Kind::Codex, ProviderId::CODEX),
            (Tool::Agy, Kind::Antigravity, ProviderId::ANTIGRAVITY),
        ] {
            let ready = self
                .active_in(kind, active(provider).as_deref(), &org)
                .then(|| quota::readiness(quota::load(&self.quota_dir, tool).as_ref(), tool, now));
            tools.insert(tool.raw().to_string(), self.tool_value(ready));
        }
        json!({"organization": org, "tools": tools})
    }

    fn tool_value(&self, ready: Option<Ready>) -> Value {
        match ready {
            Some(r) => ready_value(&r),
            None => json!({"usable": null, "reason": "other_organization", "pools": []}),
        }
    }

    fn orgs(&self, caller: &Caller) -> Value {
        let Some((org, _)) = self.mine(caller) else {
            return no_session(json!({"organization": null, "others": 0}));
        };
        let view = self.orgs.view();
        let all = view.organizations();
        let mine = all.iter().find(|o| o.id == org);
        json!({
            "organization": mine.map(|o| json!({
                "id": o.id,
                "name": o.name,
                "color": o.color,
                "folders": o.folders,
                "personal": o.id == PERSONAL_ID,
            })),
            "others": all.iter().filter(|o| o.id != org).count(),
        })
    }
}

fn own_item(item: &Item) -> Value {
    json!({
        "id": item.id, "kind": item.kind, "state": item.state,
        "title": item.title, "detail": item.detail, "createdMs": item.created_ms,
    })
}

fn short_item(item: &Item) -> Value {
    json!({"kind": item.kind, "state": item.state, "title": item.title})
}

pub fn scope_search(
    reply: &mut Value,
    org: Option<&str>,
    args: &Value,
    folder_org: &dyn Fn(&str) -> String,
) {
    let kind = args.get("kind").and_then(Value::as_str);
    let since = args.get("since").and_then(Value::as_f64);
    let Some(org) = org else {
        reply["pages"] = json!([]);
        reply["sessions"] = json!([]);
        reply["reason"] = json!("no_session");
        return;
    };
    let keep = |row: &Value| {
        row.get("folder")
            .and_then(Value::as_str)
            .is_some_and(|f| folder_org(f) == org)
    };
    let pages: Vec<Value> = reply["pages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| kind != Some("sessions") && keep(p))
        .cloned()
        .collect();
    let sessions: Vec<Value> = reply["sessions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| kind != Some("pages") && keep(s))
        .filter(|s| since.is_none_or(|t| s["modified"].as_f64().unwrap_or(0.0) >= t))
        .cloned()
        .collect();
    reply["pages"] = json!(pages);
    reply["sessions"] = json!(sessions);
    reply["organization"] = json!(org);
}

impl ReadsOps {
    pub fn search_org(&self, caller: &Caller) -> Option<String> {
        self.mine(caller).map(|(org, _)| org)
    }

    pub fn folder_org(&self, folder: &str) -> String {
        self.org_of_folder(folder)
    }
}

impl OpGroup for ReadsOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        READ_OPS.contains(&op).then_some(OpSpec::AGENT_READ)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::INBOX_MINE => Ok(self.inbox(caller)),
            ops::ACCOUNTS_SUMMARY => Ok(self.accounts(caller)),
            ops::LIMITS_SUMMARY => {
                let mut reply = self.limits(caller);
                if let Some(tool) = args.get("tool").and_then(Value::as_str) {
                    let found = reply["tools"][tool].clone();
                    reply["tool"] = if found.is_null() { json!({"usable": false, "reason": "unknown_tool"}) } else { found };
                    if let Some(pool) = args.get("pool").and_then(Value::as_str) {
                        let wanted = pool.to_lowercase();
                        let hit = reply["tool"]["pools"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .find(|p| p["name"].as_str().is_some_and(|n| n.to_lowercase().contains(&wanted)))
                            .cloned();
                        reply["pool"] = hit.unwrap_or_else(|| json!({"usable": false, "reason": "unknown_pool"}));
                    }
                }
                Ok(reply)
            }
            ops::ORGANIZATIONS_MINE => Ok(self.orgs(caller)),
            other => Err(ApiError::internal(format!("{other} has no handler"))),
        }
    }
}

#[cfg(test)]
#[path = "reads_ops_tests.rs"]
mod tests;
