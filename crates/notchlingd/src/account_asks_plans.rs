use super::follow::{End, Watch};
use super::*;
use crate::reads_ops::mask_email;
use notchling_engine::accounts::claude::ClaudeAccount;
use notchling_engine::control::MoveAllPlan;
use notchling_engine::organizations::{Source, PERSONAL_ID};
use notchling_engine::providers::ProviderId;
use notchling_engine::sessions::AgentTarget;

const NO_ACCOUNT: &str = "No account here has that id.";
const NO_LANE: &str = "No lane here has that id.";
const NO_AGENT: &str = "No session here has that id.";
const NOT_ACCOUNT_ID: &str = "That isn't an account id.";
const NOT_LANE_ID: &str = "That isn't a lane id.";
const NOT_TOOL: &str = "The tool is claude, codex or agy.";
const OTHER_MACHINE: &str = "That session is on another machine.";
const ONLY_CLAUDE: &str = "Only Claude Code accounts have lanes.";
const SIGNED_OUT: &str = "That account needs a new sign-in, in Notchling.";
const SESSION_CHARS: usize = 200;

#[derive(Clone)]
pub(super) struct Acct {
    pub tool: &'static str,
    pub provider: &'static str,
    pub kind: Kind,
    pub raw: String,
    pub email: String,
    pub needs_login: bool,
    pub switchable: bool,
    pub id: String,
}

pub(super) struct Seen {
    pub snap: Snapshot,
    pub lanes: Vec<Lane>,
    pub providers: Vec<(String, ProviderState)>,
    pub agents: Vec<Agent>,
    pub main_dir: String,
    pub home: String,
}

enum Owned {
    Account(Kind, String),
    Folder(String),
}

fn is_hex_id(text: &str, prefix: &str) -> bool {
    text.strip_prefix(prefix).is_some_and(|rest| {
        rest.len() == 16 && rest.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
    })
}

fn tool_of_word(word: &str) -> Option<(&'static str, &'static str)> {
    match word {
        "claude" => Some(("claude", ProviderId::CLAUDE_CODE)),
        "codex" => Some(("codex", ProviderId::CODEX)),
        "agy" => Some(("antigravity", ProviderId::ANTIGRAVITY)),
        _ => None,
    }
}

fn kind_of_provider(provider: &str) -> Option<(&'static str, &'static str, Kind)> {
    match provider {
        ProviderId::CLAUDE_CODE => Some(("claude", ProviderId::CLAUDE_CODE, Kind::Claude)),
        ProviderId::CODEX => Some(("codex", ProviderId::CODEX, Kind::Codex)),
        ProviderId::ANTIGRAVITY => {
            Some(("antigravity", ProviderId::ANTIGRAVITY, Kind::Antigravity))
        }
        _ => None,
    }
}

fn usage() -> ApiError {
    bad_why(USAGE, "usage")
}

fn value_error(text: &str) -> ApiError {
    bad_why(text, "bad_value")
}

fn exact<'a>(args: &'a Value, keys: &[&str]) -> Result<Vec<&'a str>, ApiError> {
    let map = args.as_object().ok_or_else(usage)?;
    if map.len() != keys.len() || !keys.iter().all(|k| map.contains_key(*k)) {
        return Err(usage());
    }
    keys.iter()
        .map(|k| map[*k].as_str().ok_or_else(usage))
        .collect()
}

fn account_id(text: &str) -> Result<(), ApiError> {
    if is_hex_id(text, "acct-") {
        Ok(())
    } else {
        Err(value_error(NOT_ACCOUNT_ID))
    }
}

fn lane_or_main(text: &str) -> Result<(), ApiError> {
    if text == "main" || is_hex_id(text, "lane-") {
        Ok(())
    } else {
        Err(value_error(NOT_LANE_ID))
    }
}

fn session_id(text: &str) -> Result<(), ApiError> {
    if text.trim().is_empty()
        || text.chars().count() > SESSION_CHARS
        || text.chars().any(char::is_control)
    {
        return Err(usage());
    }
    if text.starts_with("node:") {
        return Err(value_error(OTHER_MACHINE));
    }
    Ok(())
}

pub(super) fn shape(action: &str, args: &Value) -> Result<(), ApiError> {
    match action {
        "switch" | "remove" | "lane_create" => account_id(exact(args, &["account"])?[0]),
        "add" => tool_of_word(exact(args, &["tool"])?[0])
            .map(|_| ())
            .ok_or_else(|| value_error(NOT_TOOL)),
        "move_all" => {
            let to = exact(args, &["to"])?[0];
            if to == "main" {
                Ok(())
            } else {
                account_id(to)
            }
        }
        "move" => {
            let given = exact(args, &["session", "to"])?;
            session_id(given[0])?;
            lane_or_main(given[1])
        }
        "lane_remove" => {
            let lane = exact(args, &["lane"])?[0];
            if is_hex_id(lane, "lane-") {
                Ok(())
            } else {
                Err(value_error(NOT_LANE_ID))
            }
        }
        "lane_switch" => {
            let given = exact(args, &["lane", "account"])?;
            if !is_hex_id(given[0], "lane-") {
                return Err(value_error(NOT_LANE_ID));
            }
            account_id(given[1])
        }
        _ => Err(usage()),
    }
}

fn tilde(folder: &str, home: &str) -> String {
    match folder.strip_prefix(home) {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with('/')) => {
            format!("~{rest}")
        }
        _ => folder.to_string(),
    }
}

fn masked(email: &str, id: &str) -> String {
    let mask = mask_email(email);
    if mask.is_empty() {
        id.to_string()
    } else {
        mask
    }
}

fn running(agent: &Agent) -> bool {
    !matches!(agent.target, AgentTarget::Resume { .. })
}

fn fact(line: &str, args: Value) -> Value {
    json!({"line": line, "args": args})
}

impl Seen {
    fn on_main(&self, agent: &Agent) -> bool {
        match agent.config_dir.as_deref() {
            Some(dir) => dir == self.main_dir,
            None => agent.transcript_path.as_deref().is_some_and(|t| {
                !self.main_dir.is_empty() && std::path::Path::new(t).starts_with(&self.main_dir)
            }),
        }
    }

    fn lane_of(&self, agent: &Agent) -> Option<&Lane> {
        let dir = agent.config_dir.as_deref()?;
        self.lanes.iter().find(|l| l.path == dir)
    }

    fn in_lane<'a>(&'a self, lane: &'a Lane) -> impl Iterator<Item = &'a Agent> + 'a {
        self.agents
            .iter()
            .filter(move |a| a.config_dir.as_deref() == Some(lane.path.as_str()) && running(a))
    }

    fn claude(&self, raw: &str) -> Option<&ClaudeAccount> {
        self.snap.accounts.iter().find(|a| a.id == raw)
    }

    fn state(&self, provider: &str) -> Option<&ProviderState> {
        self.providers
            .iter()
            .find(|(p, _)| p == provider)
            .map(|(_, s)| s)
    }
}

impl AccountAsks {
    pub(super) fn seen(&self) -> Seen {
        Seen {
            snap: self.book.claude(),
            lanes: self.book.lanes(),
            providers: self.book.providers(),
            agents: self.book.agents(),
            main_dir: self.book.main_dir(),
            home: self.book.home(),
        }
    }

    fn claude_acct(&self, a: &ClaudeAccount) -> Option<Acct> {
        Some(Acct {
            tool: "claude",
            provider: ProviderId::CLAUDE_CODE,
            kind: Kind::Claude,
            raw: a.id.clone(),
            email: a.email.clone(),
            needs_login: a.needs_login,
            switchable: true,
            id: self.book.id(ProviderId::CLAUDE_CODE, &a.id)?,
        })
    }

    pub(super) fn find(&self, seen: &Seen, id: &str) -> Option<Acct> {
        if let Some(found) = seen
            .snap
            .accounts
            .iter()
            .filter_map(|a| self.claude_acct(a))
            .find(|a| a.id == id)
        {
            return Some(found);
        }
        seen.providers.iter().find_map(|(provider, state)| {
            let (tool, provider, kind) = kind_of_provider(provider)?;
            if kind == Kind::Claude {
                return None;
            }
            state.accounts.iter().find_map(|a| {
                let opaque = self.book.id(provider, &a.id)?;
                (opaque == id).then(|| Acct {
                    tool,
                    provider,
                    kind,
                    raw: a.id.clone(),
                    email: a.email.clone(),
                    needs_login: a.needs_login,
                    switchable: a.switchable,
                    id: opaque,
                })
            })
        })
    }

    fn claude_by_raw(&self, seen: &Seen, raw: &str) -> Option<Acct> {
        seen.claude(raw).and_then(|a| self.claude_acct(a))
    }

    fn bare(
        &self,
        tool: &'static str,
        provider: &'static str,
        kind: Kind,
        raw: &str,
    ) -> Option<Acct> {
        Some(Acct {
            tool,
            provider,
            kind,
            raw: raw.to_string(),
            email: String::new(),
            needs_login: false,
            switchable: true,
            id: self.book.id(provider, raw)?,
        })
    }

    fn lane_acct(&self, seen: &Seen, lane: &Lane) -> Option<Acct> {
        self.claude_by_raw(seen, &lane.account_id).or_else(|| {
            Some(Acct {
                email: lane.email.clone(),
                ..self.bare(
                    "claude",
                    ProviderId::CLAUDE_CODE,
                    Kind::Claude,
                    &lane.account_id,
                )?
            })
        })
    }

    pub(super) fn lane_by<'a>(&self, seen: &'a Seen, id: &str) -> Option<&'a Lane> {
        seen.lanes
            .iter()
            .find(|l| self.book.lane_id(&l.name).as_deref() == Some(id))
    }

    fn found(&self, seen: &Seen, id: &str) -> Result<Acct, ApiError> {
        self.find(seen, id)
            .ok_or_else(|| bad_why(NO_ACCOUNT, "no_account"))
    }

    fn claude_only(&self, acct: &Acct) -> Result<(), ApiError> {
        if acct.kind == Kind::Claude {
            Ok(())
        } else {
            Err(value_error(ONLY_CLAUDE))
        }
    }

    pub(super) fn owner(&self, kind: Kind, raw: &str) -> Option<String> {
        if self.book.closed() {
            return None;
        }
        let wanted = kind.normal(raw);
        let (_, field) = self
            .book
            .listing(kind)?
            .into_iter()
            .find(|(key, _)| *key == wanted)?;
        let resolved = self.book.resolve(kind, raw, field.as_deref());
        resolved.known.then_some(resolved.id)
    }

    fn owners(&self, owned: &[Owned], mine: &str) -> Result<(), ApiError> {
        if owned.is_empty() {
            return Ok(());
        }
        if self.book.closed() {
            return Err(unknown_owner());
        }
        let mut found = Vec::with_capacity(owned.len());
        for item in owned {
            found.push(match item {
                Owned::Account(kind, raw) => self.owner(*kind, raw).ok_or_else(unknown_owner)?,
                Owned::Folder(folder) => self.book.folder_org(folder),
            });
        }
        if found.iter().all(|org| org == mine) {
            Ok(())
        } else {
            Err(never())
        }
    }

    fn gate(&self, acting: bool) -> Result<(), ApiError> {
        if !self.book.accounts_on() {
            return Err(switch_off("accounts", ACCOUNTS_OFF));
        }
        if acting && !self.book.acting_on() {
            return Err(switch_off("acting", ACTING_OFF));
        }
        Ok(())
    }

    fn active_of(&self, seen: &Seen, acct: &Acct) -> Option<Acct> {
        if acct.kind == Kind::Claude {
            return self.main_account(seen);
        }
        let state = seen.state(acct.provider)?;
        let raw = state.active_id.as_deref()?;
        let Some(a) = state.accounts.iter().find(|a| a.id == raw) else {
            return self.bare(acct.tool, acct.provider, acct.kind, raw);
        };
        Some(Acct {
            raw: a.id.clone(),
            email: a.email.clone(),
            needs_login: a.needs_login,
            switchable: a.switchable,
            id: self.book.id(acct.provider, &a.id)?,
            ..acct.clone()
        })
    }

    fn main_account(&self, seen: &Seen) -> Option<Acct> {
        let raw = seen.snap.active.as_deref()?;
        self.claude_by_raw(seen, raw)
            .or_else(|| self.bare("claude", ProviderId::CLAUDE_CODE, Kind::Claude, raw))
    }

    fn main_folders(&self, seen: &Seen) -> Vec<Owned> {
        seen.agents
            .iter()
            .filter(|a| seen.on_main(a) && running(a))
            .map(|a| Owned::Folder(a.cwd.clone()))
            .collect()
    }

    fn lane_folders(&self, seen: &Seen, lane: &Lane) -> Vec<Owned> {
        seen.in_lane(lane)
            .map(|a| Owned::Folder(a.cwd.clone()))
            .collect()
    }

    pub(super) fn plan(&self, action: &str, args: &Value, mine: &str) -> Result<Plan, ApiError> {
        shape(action, args)?;
        let seen = self.seen();
        let text = |key: &str| args[key].as_str().unwrap_or_default().to_string();
        let base = |tool: &str| json!({"action": action, "tool": tool, "organization": mine, "args": args});
        match action {
            "switch" => self.plan_switch(&seen, &text("account"), base, mine),
            "add" => self.plan_add(&seen, &text("tool"), base, mine),
            "remove" => self.plan_remove(&seen, &text("account"), base, mine),
            "move_all" => self.plan_move_all(&seen, &text("to"), base, mine),
            "move" => self.plan_move(&seen, &text("session"), &text("to"), base, mine),
            "lane_create" => self.plan_lane_create(&seen, &text("account"), base, mine),
            "lane_remove" => self.plan_lane_remove(&seen, &text("lane"), base, mine),
            "lane_switch" => {
                self.plan_lane_switch(&seen, &text("lane"), &text("account"), base, mine)
            }
            _ => Err(usage()),
        }
    }

    fn plan_switch(
        &self,
        seen: &Seen,
        id: &str,
        base: impl Fn(&str) -> Value,
        mine: &str,
    ) -> Result<Plan, ApiError> {
        let acct = self.found(seen, id)?;
        let before = self.active_of(seen, &acct);
        let mut owned = vec![Owned::Account(acct.kind, acct.raw.clone())];
        if let Some(b) = &before {
            owned.push(Owned::Account(b.kind, b.raw.clone()));
        }
        if acct.kind == Kind::Claude {
            owned.extend(self.main_folders(seen));
        }
        self.owners(&owned, mine)?;
        self.gate(false)?;
        if before.as_ref().is_some_and(|b| b.raw == acct.raw) {
            return Err(bad_why("That account is already in use.", "already_set"));
        }
        if acct.kind == Kind::Claude {
            if seen.lanes.iter().any(|l| l.account_id == acct.raw) {
                return Err(bad_why(
                    "That account is in a lane. Close the lane first.",
                    "in_lane",
                ));
            }
            if acct.needs_login {
                return Err(forbidden(SIGNED_OUT, "signed_out"));
            }
        } else {
            if !acct.switchable {
                return Err(value_error("Notchling can't switch to that account."));
            }
            if seen.state(acct.provider).is_some_and(|s| s.busy) {
                return Err(forbidden(
                    "Another account change is under way for that tool.",
                    "busy",
                ));
            }
        }
        let mut change = base(acct.tool);
        change["account"] = json!(acct.id);
        change["before"] = json!(before.as_ref().map(|b| b.id.clone()));
        change["apply"] = if acct.kind == Kind::Claude {
            json!({"op": ops::ACCOUNTS_SWITCH, "args": {"id": acct.id}})
        } else {
            json!({"op": ops::PROVIDERS_SWITCH,
                "args": {"provider": acct.provider, "id": acct.id, "confirm": true}})
        };
        let shown = masked(&acct.email, &acct.id);
        let mut facts = vec![fact(
            "consent-account",
            json!({"tool": acct.tool, "account": shown, "organization": mine}),
        )];
        facts.push(match &before {
            Some(b) => fact(
                "consent-account-switch",
                json!({"tool": acct.tool, "account": shown, "before": masked(&b.email, &b.id)}),
            ),
            None => fact(
                "consent-account-switch-first",
                json!({"tool": acct.tool, "account": shown}),
            ),
        });
        match acct.kind {
            Kind::Codex => facts.push(fact("consent-account-switch-codex", json!({}))),
            Kind::Antigravity => facts.push(fact("consent-account-switch-antigravity", json!({}))),
            _ => {}
        }
        Ok(Plan {
            target: format!("switch:{}", acct.id),
            change,
            facts: json!(facts),
        })
    }

    fn plan_add(
        &self,
        seen: &Seen,
        word: &str,
        base: impl Fn(&str) -> Value,
        mine: &str,
    ) -> Result<Plan, ApiError> {
        let (tool, provider) = tool_of_word(word).ok_or_else(|| value_error(NOT_TOOL))?;
        self.gate(false)?;
        let busy = || forbidden("A sign-in for that tool is under way.", "busy");
        if provider != ProviderId::CLAUDE_CODE {
            if !self.book.can_add(provider) {
                return Err(value_error("Notchling can't add accounts for that tool."));
            }
            if seen.state(provider).is_some_and(|s| s.adding) {
                return Err(busy());
            }
        }
        if self.follow.adding(tool) {
            return Err(busy());
        }
        let mut change = base(tool);
        change["apply"] = if provider == ProviderId::CLAUDE_CODE {
            json!({"op": ops::ACCOUNTS_ADD_BEGIN, "args": {}})
        } else {
            json!({"op": ops::PROVIDERS_ADD_BEGIN, "args": {"provider": provider, "confirm": true}})
        };
        Ok(Plan {
            target: format!("add:{tool}"),
            change,
            facts: json!([fact(
                "consent-account-add",
                json!({"tool": tool, "organization": mine})
            )]),
        })
    }

    fn plan_remove(
        &self,
        seen: &Seen,
        id: &str,
        base: impl Fn(&str) -> Value,
        mine: &str,
    ) -> Result<Plan, ApiError> {
        let acct = self.found(seen, id)?;
        self.owners(&[Owned::Account(acct.kind, acct.raw.clone())], mine)?;
        self.gate(false)?;
        if self
            .active_of(seen, &acct)
            .is_some_and(|b| b.raw == acct.raw)
        {
            return Err(bad_why(
                "That account is in use. Switch to another first.",
                "in_use",
            ));
        }
        if acct.kind == Kind::Claude && seen.lanes.iter().any(|l| l.account_id == acct.raw) {
            return Err(bad_why(
                "That account is in a lane. Close the lane first.",
                "in_lane",
            ));
        }
        let mut change = base(acct.tool);
        change["account"] = json!(acct.id);
        change["apply"] = if acct.kind == Kind::Claude {
            json!({"op": ops::ACCOUNTS_REMOVE, "args": {"id": acct.id}})
        } else {
            json!({"op": ops::PROVIDERS_REMOVE, "args": {"provider": acct.provider, "id": acct.id}})
        };
        let shown = masked(&acct.email, &acct.id);
        Ok(Plan {
            target: format!("remove:{}", acct.id),
            change,
            facts: json!([
                fact(
                    "consent-account",
                    json!({"tool": acct.tool, "account": shown, "organization": mine})
                ),
                fact(
                    "consent-account-remove",
                    json!({"tool": acct.tool, "account": shown})
                ),
            ]),
        })
    }

    pub(super) fn translate(&self, change: &Value) -> Option<Value> {
        let seen = self.seen();
        let op = change["apply"]["op"].as_str()?;
        let mut out = change["apply"]["args"].clone();
        for key in ["id", "account", "target"] {
            if key == "id" && op == ops::SESSION_MOVE {
                continue;
            }
            if let Some(id) = out.get(key).and_then(Value::as_str).map(str::to_string) {
                out[key] = json!(self.find(&seen, &id)?.raw);
            }
        }
        if op == ops::LANES_REMOVE || op == ops::LANES_SWITCH {
            let id = out["name"].as_str()?.to_string();
            out["name"] = json!(self.lane_by(&seen, &id)?.name);
        }
        if op == ops::SESSION_MOVE {
            let to = out["to_config_dir"].as_str()?.to_string();
            out["to_config_dir"] = if to == "main" {
                json!(seen.main_dir)
            } else {
                json!(self.lane_by(&seen, &to)?.path)
            };
        }
        Some(out)
    }

    pub(super) fn adds_of(&self, row: &Consent) -> Option<&'static str> {
        if row.change["action"] != "add" {
            return None;
        }
        let provider = row.change["apply"]["args"]["provider"]
            .as_str()
            .unwrap_or(ProviderId::CLAUDE_CODE);
        kind_of_provider(provider).map(|(tool, _, _)| tool)
    }

    pub(super) fn watch_before(&self, row: &Consent) -> Option<Watch> {
        if row.change["action"] != "add" {
            return None;
        }
        let provider = row.change["apply"]["args"]["provider"]
            .as_str()
            .unwrap_or(ProviderId::CLAUDE_CODE);
        let (_, provider, kind) = kind_of_provider(provider)?;
        let mut raw: Vec<String> = if provider == ProviderId::CLAUDE_CODE {
            let snap = self.book.claude();
            snap.accounts
                .into_iter()
                .map(|a| a.id)
                .chain(snap.active)
                .chain(self.book.lanes().into_iter().map(|l| l.account_id))
                .collect()
        } else {
            self.book
                .providers()
                .into_iter()
                .filter(|(p, _)| p == provider)
                .flat_map(|(_, s)| s.accounts.into_iter().map(|a| a.id).chain(s.active_id))
                .collect()
        };
        raw.extend(
            self.book
                .listing(kind)
                .unwrap_or_default()
                .into_iter()
                .map(|(key, _)| key),
        );
        let mut before: Vec<String> = raw.iter().map(|r| kind.normal(r)).collect();
        before.sort();
        before.dedup();
        Some(if provider == ProviderId::CLAUDE_CODE {
            Watch::ClaudeAdd {
                login: None,
                before,
            }
        } else {
            Watch::ProviderAdd {
                provider: provider.to_string(),
                attempt: None,
                before,
                limit_ms: self.book.add_limit_ms(),
            }
        })
    }

    pub(super) fn after_run(
        &self,
        row: &Consent,
        watch: Option<Watch>,
        reply: &Value,
        started_ms: i64,
    ) {
        let state = reply["state"].as_str();
        if state == Some("needs_confirmation") {
            self.follow.forget(&row.id);
            self.consent
                .finish_use(row, Err("it asked for a confirmation".to_string()));
            return;
        }
        match watch {
            Some(Watch::ClaudeAdd { before, .. }) => {
                let Some(login) = reply["login"].as_str() else {
                    self.follow.forget(&row.id);
                    self.consent
                        .finish_use(row, Err("the sign-in named no login".to_string()));
                    return;
                };
                let login = Some(login.to_string());
                self.follow
                    .start(row, Watch::ClaudeAdd { login, before }, started_ms);
                return;
            }
            Some(Watch::ProviderAdd {
                provider,
                before,
                limit_ms,
                ..
            }) => {
                let Some(attempt) = reply["attempt"].as_u64() else {
                    self.follow.forget(&row.id);
                    self.consent
                        .finish_use(row, Err("the sign-in named no attempt".to_string()));
                    return;
                };
                let watch = Watch::ProviderAdd {
                    provider,
                    attempt: Some(attempt),
                    before,
                    limit_ms,
                };
                self.follow.start(row, watch, started_ms);
                return;
            }
            Some(watch) => {
                self.follow.start(row, watch, started_ms);
                return;
            }
            None => {}
        }
        let op = row.change["apply"]["op"].as_str().unwrap_or_default();
        let provider = row.change["apply"]["args"]["provider"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let late = match (state, reply["ticket"].as_u64()) {
            (Some("working"), Some(ticket)) => Some(Watch::Ticket(ticket)),
            _ if op == ops::PROVIDERS_SWITCH && state == Some("switching") => {
                Some(Watch::ProviderSwitch { provider })
            }
            _ if op == ops::PROVIDERS_REMOVE && reply["removed"] == json!(false) => {
                Some(Watch::ProviderRemove { provider })
            }
            _ => None,
        };
        match late {
            Some(watch) => self.follow.start(row, watch, self.consent.now()),
            None => {
                self.follow.release(&row.id);
                self.consent
                    .finish_use(row, Ok(self.result_of(&row.change, reply)))
            }
        }
    }

    fn result_of(&self, change: &Value, reply: &Value) -> Value {
        let action = change["action"].as_str().unwrap_or_default();
        let mut out = json!({"action": action, "done": true});
        match action {
            "switch" => out["account"] = change["account"].clone(),
            "move_all" => {
                let count = |v: &Value| match v {
                    Value::Array(a) => a.len() as u64,
                    Value::Object(m) => m.len() as u64,
                    Value::Number(n) => n.as_u64().unwrap_or(0),
                    _ => 0,
                };
                out["moved"] = json!(count(&reply["moved"]));
                out["errors"] = json!(count(&reply["errors"]));
            }
            "lane_create" => {
                out["lane"] = json!(reply["lane"]["name"]
                    .as_str()
                    .and_then(|name| self.book.lane_id(name)))
            }
            _ => {}
        }
        out
    }

    fn joined(
        &self,
        kind: Kind,
        provider: &str,
        raw: &str,
        mine: &str,
        known_before: bool,
    ) -> Value {
        let Some(id) = self.book.id(provider, raw) else {
            return Value::Null;
        };
        if self.book.closed() {
            return Value::Null;
        }
        let Some(listing) = self.book.listing(kind) else {
            return Value::Null;
        };
        let wanted = kind.normal(raw);
        let Some((_, field)) = listing.into_iter().find(|(k, _)| *k == wanted) else {
            return Value::Null;
        };
        let resolved = self.book.resolve(kind, raw, field.as_deref());
        if !resolved.known {
            return Value::Null;
        }
        if resolved.source == Source::Default && !known_before {
            if mine == PERSONAL_ID || self.book.place(kind, raw, mine) {
                return json!(id);
            }
            return Value::Null;
        }
        if resolved.id == mine {
            json!(id)
        } else {
            Value::Null
        }
    }

    pub(super) fn settle(&self, entry: follow::Entry, end: End) {
        let row = entry.row;
        let data = match end {
            End::Withdrawn => return self.consent.withdraw_use(&row),
            End::Failed => {
                return self
                    .consent
                    .finish_use(&row, Err("the account change failed".to_string()))
            }
            End::Done(data) => data,
        };
        let mine = row.organization.clone().unwrap_or_default();
        let tool = row.change["tool"].as_str().unwrap_or_default().to_string();
        let result = match entry.watch {
            Watch::ClaudeAdd { before, .. } => {
                let account = data["account"]["id"].as_str().map_or(Value::Null, |raw| {
                    let known = before.contains(&Kind::Claude.normal(raw));
                    self.joined(Kind::Claude, ProviderId::CLAUDE_CODE, raw, &mine, known)
                });
                json!({"action": "add", "done": true, "tool": tool, "account": account})
            }
            Watch::ProviderAdd {
                provider,
                attempt,
                before,
                ..
            } => {
                let kind = kind_of_provider(&provider).map(|k| k.2);
                let stored = attempt.and_then(|attempt| self.book.added(attempt));
                let account = match (stored.as_deref(), kind) {
                    (Some([one]), Some(kind)) => {
                        let known = before.contains(&kind.normal(one));
                        self.joined(kind, &provider, one, &mine, known)
                    }
                    _ => Value::Null,
                };
                json!({"action": "add", "done": true, "tool": tool, "account": account})
            }
            Watch::Ticket(_) => self.result_of(&row.change, &data["result"]),
            Watch::ProviderSwitch { .. } | Watch::ProviderRemove { .. } => {
                self.result_of(&row.change, &Value::Null)
            }
        };
        self.consent.finish_use(&row, Ok(result));
    }

    pub(super) fn lanes_summary(&self, caller: &Caller) -> Value {
        let who = self.consent.who(caller);
        let Ok(mine) = caller_of(&who) else {
            return json!({"reason": "no_session", "lanes": []});
        };
        let seen = self.seen();
        let lanes: Vec<Value> = seen
            .lanes
            .iter()
            .filter(|l| self.owner(Kind::Claude, &l.account_id).as_deref() == Some(mine.as_str()))
            .filter_map(|lane| {
                let acct = self.lane_acct(&seen, lane)?;
                Some(json!({
                    "id": self.book.lane_id(&lane.name)?,
                    "account": acct.id,
                    "name": masked(&acct.email, &acct.id),
                    "sessions": seen.in_lane(lane).count(),
                    "needsLogin": acct.needs_login,
                }))
            })
            .collect();
        json!({"organization": mine, "lanes": lanes})
    }
}

#[path = "account_asks_plans_lanes.rs"]
mod lanes;
