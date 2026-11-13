use crate::acting_ops::{now_ms, Room};
use crate::inbox_ops::InboxOps;
use crate::op_group::OpGroup;
use crate::settings_asks::{item_id, view, Applier, Ask, Asks, ASK_WAIT};
use notchling_engine::inbox::{Action, ActionId, Item, Kind as ItemKind, Source, State};
use notchling_engine::limits::mode::PROVIDERS;
use notchling_engine::limits::{LimitConfig, LimitMode};
use notchling_engine::settings::{
    spec, switch, Access, KeySpec, Settings, SwitchSpec, KEYS, SWITCHES, SWITCH_WHY,
};
use notchling_local_api::{
    ops, topics, ApiError, AuditEntry, AuditLog, Caller, ErrorCode, OpSpec, Tier,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub const SETTINGS_OPS: [&str; 4] = [
    ops::SETTINGS_LIST,
    ops::SETTINGS_GET,
    ops::SETTINGS_SET,
    ops::SETTINGS_ASKED,
];
pub const MAX_CALL_WAIT: u64 = 60;
const LIMIT_SWITCH_AT: &str = "limitSwitchAt";
const CUT_KEY: usize = 64;
const CUT_VALUE: usize = 120;

pub type SwitchStates = Arc<dyn Fn(&str) -> Option<bool> + Send + Sync>;

pub struct Writer {
    file: PathBuf,
    room: Arc<dyn Room>,
    audit: Option<AuditLog>,
    limits: Option<Arc<LimitConfig>>,
}

fn cut(text: &str, at: usize) -> String {
    if text.chars().count() <= at {
        return text.to_string();
    }
    let kept: String = text.chars().take(at).collect();
    format!("{kept}…")
}

fn shown(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn bad(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::BadRequest, message)
}

fn forbidden(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, message)
}

fn failed(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, message)
}

fn apply_limit(limits: &LimitConfig, key: &str, value: &Value) -> Result<(), String> {
    if key == LIMIT_SWITCH_AT {
        let n = value.as_i64().ok_or("limitSwitchAt is a whole number")?;
        return limits.set_switch_at(n);
    }
    match PROVIDERS.iter().find(|p| p.key == key) {
        Some(provider) => {
            let mode = value
                .as_str()
                .and_then(LimitMode::parse)
                .ok_or("a limit mode is off, notify or notifyAndSwitch")?;
            limits.set_mode(provider, mode)
        }
        None => Ok(()),
    }
}

impl Writer {
    pub fn write(
        &self,
        spec: &KeySpec,
        value: &Value,
        by: &str,
        ask: Option<&str>,
    ) -> Result<bool, String> {
        if let Some(limits) = &self.limits {
            apply_limit(limits, spec.name, value)?;
        }
        let mut store = Settings::open(&self.file).map_err(|e| e.to_string())?;
        let before = store.get(spec.name);
        store
            .set(spec.name, value.clone())
            .map_err(|e| e.to_string())?;
        let mut event = json!({"key": spec.name, "value": value, "by": by});
        if let Some(ask) = ask {
            event["ask"] = json!(ask);
        }
        self.room
            .publish(topics::SETTINGS, topics::SETTINGS_CHANGED, &event);
        Ok(before.as_ref() != Some(value))
    }

    pub fn audit(
        &self,
        tier: Tier,
        peer: Option<&std::path::Path>,
        result: &str,
        key: &str,
        value: &Value,
    ) {
        let Some(audit) = &self.audit else {
            return;
        };
        let reason = format!(
            "{}={}",
            cut(key, CUT_KEY),
            cut(&value.to_string(), CUT_VALUE)
        );
        let entry =
            AuditEntry::now(ops::SETTINGS_SET, Some(tier), peer, result).with_reason(Some(reason));
        let _ = audit.append(&entry);
    }
}

impl Applier for Writer {
    fn apply(&self, id: &str, ask: &Ask) -> Result<(), String> {
        let spec = spec(ask.key).ok_or("the setting is gone")?;
        self.write(spec, &ask.value, "agent", Some(id)).map(|_| ())
    }

    fn ended(&self, _id: &str, ask: &Ask) {
        let result = match ask.state.as_str() {
            "applied" => "allowed",
            other => other,
        };
        self.audit(
            Tier::Agent,
            ask.peer.as_deref(),
            result,
            ask.key,
            &ask.value,
        );
    }
}

pub struct SettingsOps {
    writer: Arc<Writer>,
    switches: SwitchStates,
    asks: Arc<Asks>,
    inbox: Option<Arc<InboxOps>>,
    ask_wait: Duration,
}

impl SettingsOps {
    pub fn new(data_dir: PathBuf, room: Arc<dyn Room>, switches: SwitchStates) -> SettingsOps {
        SettingsOps {
            writer: Arc::new(Writer {
                file: data_dir.join("settings.json"),
                room,
                audit: None,
                limits: None,
            }),
            switches,
            asks: Asks::new(),
            inbox: None,
            ask_wait: ASK_WAIT,
        }
    }

    pub fn with_audit(mut self, audit: AuditLog) -> SettingsOps {
        if let Some(writer) = Arc::get_mut(&mut self.writer) {
            writer.audit = Some(audit);
        }
        self
    }

    pub fn with_limits(mut self, limits: Arc<LimitConfig>) -> SettingsOps {
        if let Some(writer) = Arc::get_mut(&mut self.writer) {
            writer.limits = Some(limits);
        }
        self
    }

    pub fn with_inbox(mut self, inbox: Arc<InboxOps>) -> SettingsOps {
        self.inbox = Some(inbox);
        self
    }

    pub fn with_ask_wait(mut self, wait: Duration) -> SettingsOps {
        self.ask_wait = wait;
        self
    }

    pub fn close(&self) {
        self.asks.close();
    }

    fn store(&self) -> Option<Settings> {
        Settings::open(&self.writer.file).ok()
    }

    fn entry(&self, spec: &KeySpec, store: Option<&Settings>, full: bool) -> Value {
        let mut e = json!({
            "key": spec.name,
            "title": spec.shown_title(),
            "description": spec.about,
            "kind": spec.kind_name(),
            "class": spec.access.as_str(),
            "why": spec.why,
        });
        if !spec.allowed.is_empty() {
            e["allowed"] = json!(spec.allowed);
        }
        if let Some((low, high)) = spec.range {
            e["range"] = json!([low, high]);
        }
        if full || spec.access.shown() {
            e["value"] = store.and_then(|s| s.get(spec.name)).unwrap_or(Value::Null);
            e["default"] = spec.default_value().unwrap_or(Value::Null);
        }
        e
    }

    fn switch_entry(&self, s: &SwitchSpec) -> Value {
        json!({
            "key": s.key,
            "title": s.title,
            "description": s.about,
            "kind": "bool",
            "class": Access::Never.as_str(),
            "why": SWITCH_WHY,
            "switch": true,
            "value": (self.switches)(s.name),
        })
    }

    fn list(&self, caller: &Caller) -> Value {
        let store = self.store();
        let full = caller.tier == Tier::App;
        let mut all: Vec<Value> = SWITCHES.iter().map(|s| self.switch_entry(s)).collect();
        all.extend(KEYS.iter().map(|k| self.entry(k, store.as_ref(), full)));
        json!({"settings": all})
    }

    fn get(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let key = args
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("settings.get takes {key}"))?;
        if let Some(s) = switch(key) {
            return Ok(self.switch_entry(s));
        }
        let spec = spec(key).ok_or_else(|| bad(unknown(key)))?;
        let store = self.store();
        let mut entry = self.entry(spec, store.as_ref(), caller.tier == Tier::App);
        let pending: Vec<Value> = self
            .asks
            .waiting_for(spec.name)
            .iter()
            .map(|(id, ask)| json!({"id": id, "value": ask.value}))
            .collect();
        if !pending.is_empty() {
            entry["pending"] = json!(pending);
        }
        Ok(entry)
    }

    fn set(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let usage = "settings.set takes {key, value, wait?: seconds}";
        let key = args
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| bad(usage))?;
        let raw = args.get("value").cloned().ok_or_else(|| bad(usage))?;
        let wait = args
            .get("wait")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(MAX_CALL_WAIT);
        let peer = caller.peer.as_deref();
        let audit = |result: &str, value: &Value| {
            self.writer.audit(caller.tier, peer, result, key, value);
        };
        if let Some(s) = switch(key) {
            audit("refused", &raw);
            return Err(forbidden(format!(
                "{} ({}) is a switch: only the user turns it on or off, in Notchling's Settings, under Daemon.",
                s.title, s.key
            )));
        }
        let Some(spec) = spec(key) else {
            audit("unknown_key", &raw);
            return Err(bad(unknown(key)));
        };
        if !spec.access.shown() {
            audit("refused", &raw);
            return Err(forbidden(format!(
                "{} ({}) can't be changed by an agent: {}. The user changes it in Notchling.",
                spec.shown_title(),
                spec.name,
                spec.why
            )));
        }
        let value = spec.read_value(raw);
        if let Err(why) = spec.check(&value) {
            audit("bad_value", &value);
            return Err(bad(format!("{why}.")));
        }
        let app = caller.tier == Tier::App;
        if app || spec.access == Access::Free {
            let by = if app { "app" } else { "agent" };
            let changed = self.writer.write(spec, &value, by, None).map_err(|why| {
                audit("failed", &value);
                failed(format!("{} couldn't be set: {why}", spec.name))
            })?;
            audit("set", &value);
            return Ok(
                json!({"key": spec.name, "value": value, "state": "applied", "changed": changed}),
            );
        }
        self.ask(caller, spec, value, Duration::from_secs(wait))
    }

    fn ask(
        &self,
        caller: &Caller,
        spec: &'static KeySpec,
        value: Value,
        wait: Duration,
    ) -> Result<Value, ApiError> {
        let peer = caller.peer.as_deref();
        let Some(inbox) = self
            .inbox
            .as_ref()
            .filter(|_| self.writer.room.can_follow(topics::INBOX))
        else {
            self.writer
                .audit(caller.tier, peer, "no_app", spec.name, &value);
            return Err(ApiError::new(
                ErrorCode::NeedsApproval,
                format!(
                    "Changing {} needs the user's approval in Notchling's Inbox, and the app isn't open. Ask the user to open Notchling, or to change it there.",
                    spec.shown_title()
                ),
            ));
        };
        let id = notchling_engine::vault::random_id("setting-")
            .ok_or_else(|| ApiError::internal("no randomness for an ask id"))?;
        let ask = Ask::new(spec.name, value.clone(), caller.peer.clone(), self.ask_wait);
        self.asks.start(self.writer.clone(), Some(inbox.clone()));
        if !self.asks.open(&id, ask) {
            self.writer
                .audit(caller.tier, peer, "full", spec.name, &value);
            return Err(failed(
                "Too many changes wait for the user's approval; try again later.",
            ));
        }
        let before = self
            .store()
            .and_then(|s| s.get(spec.name))
            .map(|v| shown(&v))
            .unwrap_or_else(|| "unset".into());
        inbox.raise(
            self.item(&id, spec, &value, &before),
            Some(self.asks.responder(&id)),
        );
        self.writer
            .audit(caller.tier, peer, "asked", spec.name, &value);
        let now = self
            .asks
            .wait(&id, wait)
            .ok_or_else(|| failed("The ask ended."))?;
        Ok(view(&id, &now))
    }

    fn item(&self, id: &str, spec: &KeySpec, value: &Value, before: &str) -> Item {
        let created_ms = now_ms();
        let lasts = i64::try_from(self.ask_wait.as_millis()).unwrap_or(i64::MAX);
        let action = |id, label: &str, primary| Action {
            id,
            label: label.to_string(),
            primary,
        };
        Item {
            id: item_id(id),
            kind: ItemKind::SettingChange,
            title: format!("Set {} to {}", spec.shown_title(), shown(value)),
            detail: format!("An agent asks. {}. Now {before}.", spec.about),
            source: Source::default(),
            created_ms,
            expires_ms: Some(created_ms.saturating_add(lasts)),
            actions: vec![
                action(ActionId::Allow, "Allow", true),
                action(ActionId::Deny, "Deny", false),
            ],
            state: State::Open,
            answer: None,
            answered_by: None,
            seen: false,
            open: None,
            closed_ms: None,
        }
    }

    fn asked(&self, args: &Value) -> Result<Value, ApiError> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("settings.asked takes {id, wait?: seconds}"))?;
        let wait = args
            .get("wait")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(MAX_CALL_WAIT);
        let ask = self
            .asks
            .wait(id, Duration::from_secs(wait))
            .ok_or_else(|| failed("No such ask: it may have ended over ten minutes ago."))?;
        Ok(view(id, &ask))
    }
}

pub fn unknown(key: &str) -> String {
    let word = key.trim().to_ascii_lowercase();
    let near: Vec<&str> = KEYS
        .iter()
        .filter(|k| k.access.shown() && !word.is_empty())
        .filter(|k| {
            let name = k.name.to_ascii_lowercase();
            name.contains(&word)
                || word.contains(&name)
                || k.title.to_ascii_lowercase().contains(&word)
        })
        .map(|k| k.name)
        .take(3)
        .collect();
    let maybe = if near.is_empty() {
        String::new()
    } else {
        format!(" Did you mean {}?", near.join(", "))
    };
    format!(
        "No setting named {}.{maybe} `notchling settings list` shows them all.",
        cut(key, CUT_KEY)
    )
}

impl OpGroup for SettingsOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        SETTINGS_OPS.contains(&op).then_some(OpSpec::AGENT)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::SETTINGS_LIST => Ok(self.list(caller)),
            ops::SETTINGS_GET => self.get(caller, args),
            ops::SETTINGS_SET => self.set(caller, args),
            ops::SETTINGS_ASKED => self.asked(args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    fn topics(&self) -> &[&'static str] {
        &[topics::SETTINGS]
    }
}

#[cfg(test)]
#[path = "settings_ops_tests.rs"]
mod tests;
