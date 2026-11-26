use crate::account_ids::AccountIds;
use crate::inbox_ops::InboxOps;
use crate::limits_ops::LimitsOps;
use crate::op_group::OpGroup;
use crate::providers_ops::ProvidersOps;
use notchling_engine::accounts::claude::ClaudeAccounts;
use notchling_engine::delegate::options::Tool;
use notchling_engine::delegate::quota::{self, Ready, Window};
use notchling_engine::inbox::Item;
use notchling_engine::organizations::{Kind, Organizations, PERSONAL_ID};
use notchling_engine::providers::ProviderId;
use notchling_local_api::{bound_identity, ops, AgentIdentity, ApiError, Caller, OpSpec, Tier};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

#[path = "reads_history.rs"]
mod history;

pub const READ_OPS: [&str; 5] = [
    ops::SESSIONS_HISTORY,
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
    claude: Arc<ClaudeAccounts>,
    limits: Arc<LimitsOps>,
    quota_dir: PathBuf,
    ids: AccountIds,
    now: Clock,
    history: notchling_engine::sessions::history::History,
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

fn verified_agent(caller: &Caller) -> Option<AgentIdentity> {
    if caller.tier == Tier::App {
        return None;
    }
    bound_identity().filter(|who| who.verified)
}

pub fn verified_org(caller: &Caller) -> Option<String> {
    verified_agent(caller)?.organization
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
        claude: Arc<ClaudeAccounts>,
        limits: Arc<LimitsOps>,
        daemon_dir: &Path,
        now: Clock,
    ) -> ReadsOps {
        ReadsOps {
            orgs,
            home: home.to_path_buf(),
            sessions,
            inbox,
            providers,
            claude,
            limits,
            quota_dir: daemon_dir.join("accounts"),
            ids: AccountIds::in_dir(daemon_dir),
            now,
            history: notchling_engine::sessions::history::History::new(),
        }
    }

    fn org_of_folder(&self, folder: &str) -> String {
        self.orgs
            .view()
            .folder_here(Path::new(folder), &self.home)
            .organization
    }

    fn org_placing(&self, other: &str) -> Option<String> {
        let folders = {
            let list = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
            crate::identity::folders_naming(&list, other)
        };
        let mut orgs = folders.iter().map(|f| self.org_of_folder(f));
        let first = orgs.next()?;
        orgs.all(|o| o == first).then_some(first)
    }

    fn mine(&self, caller: &Caller) -> Option<(String, Option<String>)> {
        let who = verified_agent(caller)?;
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
            let theirs = item
                .source
                .session
                .as_deref()
                .and_then(|s| self.org_placing(s));
            if theirs.as_deref() == Some(org.as_str()) {
                same.push(short_item(&item));
            } else {
                others += 1;
            }
        }
        json!({"own": own, "organization": same, "otherOrganizations": others})
    }

    fn owner_of(&self, kind: Kind, raw: &str) -> Option<String> {
        let view = self.orgs.view();
        if view.is_closed() {
            return None;
        }
        let wanted = kind.normal(raw);
        let (_, field) = self
            .orgs
            .listing(kind)?
            .into_iter()
            .find(|(key, _)| *key == wanted)?;
        let resolved = view.resolve(kind, raw, field.as_deref());
        resolved.known.then_some(resolved.id)
    }

    fn claude_rows(&self, org: &str) -> Vec<Value> {
        let snap = self.claude.snapshot();
        let lanes = self.claude.env().lanes.all();
        snap.accounts
            .iter()
            .filter(|a| self.owner_of(Kind::Claude, &a.id).as_deref() == Some(org))
            .filter_map(|a| {
                let lane = lanes
                    .iter()
                    .find(|l| l.account_id == a.id)
                    .map(|l| self.ids.lane(&l.name))
                    .map_or(Some(None), |id| id.map(Some))?;
                Some(json!({
                    "id": self.opaque_account_id(ProviderId::CLAUDE_CODE, &a.id)?,
                    "name": mask_email(&a.email),
                    "organization": org,
                    "active": snap.active.as_deref() == Some(a.id.as_str()),
                    "plan": a.plan,
                    "needsLogin": a.needs_login,
                    "lane": lane,
                }))
            })
            .collect()
    }

    fn accounts(&self, caller: &Caller) -> Value {
        let Some((org, _)) = self.mine(caller) else {
            return no_session(json!({"tools": {}}));
        };
        let mut tools = serde_json::Map::new();
        tools.insert("claude".to_string(), json!(self.claude_rows(&org)));
        for (provider, state) in self.providers.states() {
            let Some((kind, tool)) = kind_of(&provider) else {
                continue;
            };
            if kind == Kind::Claude {
                continue;
            }
            let rows: Vec<Value> = state
                .accounts
                .iter()
                .filter(|a| self.owner_of(kind, &a.id).as_deref() == Some(org.as_str()))
                .filter_map(|a| {
                    Some(json!({
                        "id": self.opaque_account_id(&provider, &a.id)?,
                        "name": mask_email(&a.email),
                        "organization": org,
                        "active": state.active_id.as_deref() == Some(a.id.as_str()),
                        "plan": a.plan,
                        "needsLogin": a.needs_login,
                    }))
                })
                .collect();
            tools.insert(tool.to_string(), json!(rows));
        }
        json!({"organization": org, "tools": tools})
    }

    pub fn opaque_account_id(&self, provider: &str, raw: &str) -> Option<String> {
        self.ids.id(provider, raw)
    }

    pub fn account_for_opaque(&self, caller: &Caller, tool: &str, opaque: &str) -> Option<String> {
        let (org, _) = self.mine(caller)?;
        self.providers
            .states()
            .into_iter()
            .find_map(|(provider, state)| {
                let (kind, name) = kind_of(&provider)?;
                if name != tool {
                    return None;
                }
                state
                    .accounts
                    .iter()
                    .filter(|a| self.orgs.resolve(kind, &a.id).id == org)
                    .find(|a| self.opaque_account_id(&provider, &a.id).as_deref() == Some(opaque))
                    .map(|a| a.id.clone())
            })
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
        let claude_account = states
            .iter()
            .find(|(p, _)| p == ProviderId::CLAUDE_CODE)
            .and_then(|(_, s)| {
                let id = s.active_id.clone()?;
                let needs_login = s.accounts.iter().any(|a| a.id == id && a.needs_login);
                Some((id, needs_login))
            })
            .filter(|(id, _)| self.active_in(Kind::Claude, Some(id), &org));
        let ready = claude_account.map(|(id, needs_login)| {
            let seen = self
                .limits
                .claude_observed()
                .filter(|o| o.active_id.as_deref() == Some(id.as_str()))
                .unwrap_or_default();
            let windows = seen
                .windows
                .iter()
                .map(|(label, percent, resets_at)| Window {
                    label: label.clone(),
                    percent: *percent,
                    resets_at: *resets_at,
                })
                .collect();
            quota::readiness_of_windows(seen.checked, !needs_login, windows, now)
        });
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

pub(crate) fn own_item(item: &Item) -> Value {
    let mut shown = json!({
        "id": item.id, "kind": item.kind, "state": item.state,
        "title": item.title, "detail": item.detail, "createdMs": item.created_ms,
    });
    if let Some(outcome) = &item.outcome {
        shown["outcome"] = json!({
            "state": outcome.state, "reason": outcome.reason, "atMs": outcome.at_ms,
        });
    }
    if item.is_blocking() {
        shown["blocking"] = json!(true);
        shown["waitingSince"] = json!(item.waiting_since);
        shown["waiter"] = json!(item.waiter);
    }
    if let Some(place) = &item.place {
        shown["place"] = json!({
            "machine": place.machine, "machineName": place.machine_name,
            "session": place.session, "sessionName": place.session_name,
            "branch": place.branch,
        });
    }
    shown
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
            .filter(|f| !f.is_empty())
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

pub fn no_session_search(args: &Value) -> Value {
    let mut reply = json!({});
    scope_search(&mut reply, None, args, &|_| String::new());
    reply
}

fn pick_pool(tool: &Value, wanted: &str) -> Value {
    let pools = tool["pools"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let named = |p: &Value| p["name"].as_str().map(str::to_lowercase);
    let wanted = wanted.to_lowercase();
    if let Some(hit) = pools.iter().find(|p| named(p).is_some_and(|n| n == wanted)) {
        return hit.clone();
    }
    let near: Vec<&Value> = pools
        .iter()
        .filter(|p| named(p).is_some_and(|n| n.contains(&wanted)))
        .collect();
    let names =
        |list: &[&Value]| -> Vec<Value> { list.iter().map(|p| p["name"].clone()).collect() };
    match near.as_slice() {
        [one] => (*one).clone(),
        [] if pools.is_empty() => json!({
            "usable": tool["usable"].as_bool().unwrap_or(false),
            "reason": tool["reason"],
        }),
        [] => {
            let all: Vec<&Value> = pools.iter().collect();
            json!({"usable": false, "reason": "unknown_pool", "pools": names(&all)})
        }
        _ => json!({"usable": false, "reason": "ambiguous_pool", "pools": names(&near)}),
    }
}

impl ReadsOps {
    pub fn search_org(&self, caller: &Caller) -> Option<String> {
        self.mine(caller).map(|(org, _)| org)
    }

    pub fn folder_org(&self, folder: &str) -> String {
        self.org_of_folder(folder)
    }

    pub fn folder_claim(&self, path: &str) -> Option<String> {
        let found = self.orgs.view().folder_here(Path::new(path), &self.home);
        found.folder.map(|_| found.organization)
    }

    pub fn view_only_personal(&self) -> bool {
        self.orgs.view().only_personal()
    }
}

impl OpGroup for ReadsOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        READ_OPS.contains(&op).then_some(OpSpec::AGENT_READ)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::SESSIONS_HISTORY => self.history(caller, args),
            ops::INBOX_MINE => Ok(self.inbox(caller)),
            ops::ACCOUNTS_SUMMARY => Ok(self.accounts(caller)),
            ops::LIMITS_SUMMARY => {
                let mut reply = self.limits(caller);
                if let Some(tool) = args.get("tool").and_then(Value::as_str) {
                    let found = reply["tools"][tool].clone();
                    reply["tool"] = if reply["reason"] == json!("no_session") {
                        json!({"usable": false, "reason": "no_session"})
                    } else if found.is_null() {
                        let known: Vec<&String> = reply["tools"]
                            .as_object()
                            .map(|m| m.keys().collect())
                            .unwrap_or_default();
                        json!({"usable": false, "reason": "unknown_tool", "tools": known})
                    } else {
                        found
                    };
                    if let Some(pool) = args.get("pool").and_then(Value::as_str) {
                        reply["pool"] = pick_pool(&reply["tool"], pool);
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
