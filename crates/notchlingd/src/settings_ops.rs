use crate::acting_ops::{now_ms, Room};
use crate::cluster_failover_setting::{parse, FailoverSink, KEY as FAILOVER_KEY};
use crate::consent_ops::{options, ConsentOps, Handler, Opening};
use crate::consent_wire::ask_message;
use crate::contract::Failure;
use crate::late_emit::Events;
use crate::op_group::OpGroup;
use notchling_engine::consent::{Consent, SETTING};
use notchling_engine::limits::mode::PROVIDERS;
use notchling_engine::limits::{LimitConfig, LimitMode};
use notchling_engine::settings::{
    log_value, spec, switch, Access, KeySpec, Settings, SwitchSpec, KEYS, SWITCHES, SWITCH_WHY,
};
use notchling_events::kinds::choices::{Answerer, ConsentThen};
use notchling_events::kinds::settings::{AgentSetResult, SettingClass, SettingsAgent};
use notchling_events::spec::Outcome;
use notchling_events::{scope, Device, Event, Ref};
use notchling_local_api::{ops, topics, ApiError, Caller, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
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

pub type SwitchStates = Arc<dyn Fn(&str) -> Option<bool> + Send + Sync>;

pub struct Writer {
    file: PathBuf,
    room: Arc<dyn Room>,
    events: Option<Events>,
    limits: Option<Arc<LimitConfig>>,
    failover: OnceLock<Arc<dyn FailoverSink>>,
}

fn cut(text: &str, at: usize) -> String {
    if text.chars().count() <= at {
        return text.to_string();
    }
    let kept: String = text.chars().take(at).collect();
    format!("{kept}…")
}

fn class_of(spec: &KeySpec) -> Option<SettingClass> {
    match spec.access {
        Access::Free => Some(SettingClass::Free),
        Access::Ask => Some(SettingClass::Ask),
        Access::Never => Some(SettingClass::Never),
        Access::Unset => None,
    }
}

fn outcome_of(result: AgentSetResult) -> Outcome {
    match result {
        AgentSetResult::Set | AgentSetResult::Allowed => Outcome::Ok,
        AgentSetResult::Asked => Outcome::Waiting,
        AgentSetResult::Denied
        | AgentSetResult::Expired
        | AgentSetResult::Refused
        | AgentSetResult::UnknownKey
        | AgentSetResult::BadValue
        | AgentSetResult::NoApp => Outcome::Refused,
        AgentSetResult::Full | AgentSetResult::Failed | AgentSetResult::Other => Outcome::Failed,
    }
}

pub struct Said<'a> {
    pub key: &'a str,
    pub class: Option<SettingClass>,
    pub value: &'a Value,
    pub result: AgentSetResult,
    pub by: Option<Answerer>,
    pub device: Option<&'a str>,
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
        if spec.name == FAILOVER_KEY {
            if let Some(sink) = self.failover.get() {
                let failover = parse(value).ok_or("failover is off, 30m or 1h")?;
                sink.set(failover)?;
            }
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

    pub fn record(&self, said: &Said, commit: bool) -> bool {
        let Some(events) = &self.events else {
            return true;
        };
        let logged = log_value(said.key, said.value, &events.salt());
        let event = Event::new(
            &SettingsAgent {
                key: logged.key,
                key_hash: logged.key_hash,
                class: said.class,
                value_flag: logged.value_flag,
                value_number: logged.value_number,
                value_choice: logged.value_choice,
                value_chars: logged.value_chars,
                value_hash: logged.value_hash,
                result: said.result,
                answered_by: said.by,
                device: said.device.map(Ref::<Device>::new),
            },
            outcome_of(said.result),
            now_ms(),
        );
        if commit {
            events.commit(event)
        } else {
            events.push(event);
            true
        }
    }
}

impl Handler for Writer {
    fn recheck(&self, row: &Consent) -> Result<(), String> {
        let before = row.change.get("before").cloned().unwrap_or(Value::Null);
        let store = Settings::open(&self.file).map_err(|e| e.to_string())?;
        let now = store.get(&row.target).unwrap_or(Value::Null);
        if now == before {
            Ok(())
        } else {
            Err("the setting changed meanwhile".to_string())
        }
    }

    fn apply(
        &self,
        row: &Consent,
        by: Option<Answerer>,
        device: Option<&str>,
    ) -> Result<Value, String> {
        let spec = spec(&row.target).ok_or("the setting is gone")?;
        let after = row.change.get("after").cloned().unwrap_or(Value::Null);
        let said = Said {
            key: spec.name,
            class: class_of(spec),
            value: &after,
            result: AgentSetResult::Allowed,
            by,
            device,
        };
        if !self.record(&said, true) {
            return Err("the audit log can't be written".to_string());
        }
        let changed = self.write(spec, &after, "agent", Some(&row.id))?;
        Ok(json!({"key": spec.name, "value": after, "changed": changed}))
    }

    fn ended(&self, row: &Consent, then: ConsentThen, device: Option<&str>) {
        let result = match then {
            ConsentThen::Denied => AgentSetResult::Denied,
            ConsentThen::Expired | ConsentThen::Withdrawn | ConsentThen::Cancelled => {
                AgentSetResult::Expired
            }
            ConsentThen::Failed => AgentSetResult::Failed,
            _ => return,
        };
        let Some(spec) = spec(&row.target) else {
            return;
        };
        let after = row.change.get("after").cloned().unwrap_or(Value::Null);
        let said = Said {
            key: spec.name,
            class: class_of(spec),
            value: &after,
            result,
            by: row.by.as_deref().and_then(Answerer::from_name),
            device,
        };
        self.record(&said, false);
    }
}

pub struct SettingsOps {
    writer: Arc<Writer>,
    switches: SwitchStates,
    consent: Option<Arc<ConsentOps>>,
}

impl SettingsOps {
    pub fn new(data_dir: PathBuf, room: Arc<dyn Room>, switches: SwitchStates) -> SettingsOps {
        SettingsOps {
            writer: Arc::new(Writer {
                file: data_dir.join("settings.json"),
                room,
                events: None,
                limits: None,
                failover: OnceLock::new(),
            }),
            switches,
            consent: None,
        }
    }

    pub fn with_events(mut self, events: Events) -> SettingsOps {
        if let Some(writer) = Arc::get_mut(&mut self.writer) {
            writer.events = Some(events);
        }
        self
    }

    pub fn with_limits(mut self, limits: Arc<LimitConfig>) -> SettingsOps {
        if let Some(writer) = Arc::get_mut(&mut self.writer) {
            writer.limits = Some(limits);
        }
        self
    }

    pub fn bind_failover(&self, sink: Arc<dyn FailoverSink>) {
        let _ = self.writer.failover.set(sink);
    }

    pub fn with_consent(mut self, consent: Arc<ConsentOps>) -> SettingsOps {
        self.consent = Some(consent);
        self
    }

    pub fn writer(&self) -> Arc<Writer> {
        self.writer.clone()
    }

    pub fn close(&self) {}

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
            .consent
            .as_ref()
            .map(|c| c.pending_for(caller, SETTING, spec.name))
            .unwrap_or_default()
            .iter()
            .map(|row| json!({"id": row.id, "value": row.change.get("after")}))
            .collect();
        if !pending.is_empty() {
            entry["pending"] = json!(pending);
        }
        Ok(entry)
    }

    fn dry_reply(
        &self,
        would: &str,
        change: Value,
        request: Value,
        error: Option<&ApiError>,
    ) -> Value {
        let error = error
            .map(|e| {
                Failure::from_api(Some(ops::SETTINGS_SET), e.clone()).to_value()["error"].clone()
            })
            .unwrap_or(Value::Null);
        json!({"dryRun": true, "op": ops::SETTINGS_SET, "would": would,
               "change": change, "request": request, "error": error})
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
        let agent = caller.tier == Tier::Agent;
        let dry = agent && args.get("dryRun").and_then(Value::as_bool) == Some(true);
        let note = |class: Option<SettingClass>, result: AgentSetResult, value: &Value| {
            if agent && !dry {
                self.writer.record(
                    &Said {
                        key,
                        class,
                        value,
                        result,
                        by: None,
                        device: None,
                    },
                    false,
                );
                scope::mark_recorded();
            }
        };
        let refuse = |e: ApiError, change: Value| -> Result<Value, ApiError> {
            if dry {
                Ok(self.dry_reply("refuse", change, Value::Null, Some(&e)))
            } else {
                Err(e)
            }
        };
        if let Some(s) = switch(key) {
            note(Some(SettingClass::Never), AgentSetResult::Refused, &raw);
            return refuse(
                forbidden(format!(
                    "{} ({}) is a switch: only the user turns it on or off, in Notchling's Settings, under Daemon.",
                    s.title, s.key
                ))
                .with_reason("never")
                .with_field("never", json!(true)),
                json!({"key": key}),
            );
        }
        let Some(spec) = spec(key) else {
            note(None, AgentSetResult::UnknownKey, &raw);
            return refuse(
                bad(unknown(key)).with_reason("unknown_key"),
                json!({"key": key}),
            );
        };
        if !spec.access.shown() {
            note(class_of(spec), AgentSetResult::Refused, &raw);
            return refuse(
                forbidden(format!(
                    "{} ({}) can't be changed by an agent: {}. The user changes it in Notchling.",
                    spec.shown_title(),
                    spec.name,
                    spec.why
                ))
                .with_reason("never")
                .with_field("never", json!(true)),
                json!({"key": key}),
            );
        }
        let value = spec.read_value(raw);
        if let Err(why) = spec.check(&value) {
            note(class_of(spec), AgentSetResult::BadValue, &value);
            return refuse(
                bad(format!("{why}.")).with_reason("bad_value"),
                json!({"key": key}),
            );
        }
        let app = caller.tier == Tier::App;
        let before = self
            .store()
            .and_then(|s| s.get(spec.name))
            .unwrap_or(Value::Null);
        if dry && spec.access == Access::Free {
            return Ok(self.dry_reply(
                "apply",
                json!({"key": spec.name, "before": before, "after": value}),
                Value::Null,
                None,
            ));
        }
        if app || spec.access == Access::Free {
            let by = if app { "app" } else { "agent" };
            if !app {
                let said = Said {
                    key: spec.name,
                    class: class_of(spec),
                    value: &value,
                    result: AgentSetResult::Set,
                    by: None,
                    device: None,
                };
                scope::mark_recorded();
                if !self.writer.record(&said, true) {
                    return Err(failed("the audit log can't be written"));
                }
            }
            let changed = self.writer.write(spec, &value, by, None).map_err(|why| {
                note(class_of(spec), AgentSetResult::Failed, &value);
                failed(format!("{} couldn't be set: {why}", spec.name))
            })?;
            return Ok(
                json!({"key": spec.name, "value": value, "state": "applied", "changed": changed}),
            );
        }
        self.ask(
            caller,
            spec,
            (value, before),
            (args, dry),
            Duration::from_secs(wait),
        )
    }

    fn opening(spec: &'static KeySpec, value: &Value, before: &Value, args: &Value) -> Opening {
        let (reason, expires_in_ms, key) = options(args);
        let text = spec.allowed.is_empty()
            && matches!(spec.kind_name(), "string" | "list" | "object" | "json");
        Opening {
            kind: SETTING,
            target: spec.name.to_string(),
            change: json!({"key": spec.name, "title": spec.shown_title(), "before": before,
                           "after": value, "text": text}),
            request: json!({"op": ops::SETTINGS_SET, "key": spec.name, "value": value}),
            facts: json!([{"line": "consent-setting-change",
                           "args": {"key": spec.name, "title": spec.shown_title()}}]),
            reason,
            expires_in_ms,
            key,
        }
    }

    fn ask(
        &self,
        caller: &Caller,
        spec: &'static KeySpec,
        (value, before): (Value, Value),
        (args, dry): (&Value, bool),
        wait: Duration,
    ) -> Result<Value, ApiError> {
        let said = |result: AgentSetResult, value: &Value| {
            self.writer.record(
                &Said {
                    key: spec.name,
                    class: class_of(spec),
                    value,
                    result,
                    by: None,
                    device: None,
                },
                false,
            );
            scope::mark_recorded();
        };
        let consent = self
            .consent
            .as_ref()
            .ok_or_else(|| ApiError::internal("settings has no consent ops"))?;
        let opening = Self::opening(spec, &value, &before, args);
        if dry {
            let change = opening.change.clone();
            return match consent.would(&opening) {
                Ok(request) => Ok(self.dry_reply("ask", change, request, None)),
                Err(e) => Ok(self.dry_reply("refuse", change, Value::Null, Some(&e))),
            };
        }
        let opened = consent.open(caller, opening).inspect_err(|_| {
            said(AgentSetResult::Failed, &value);
        })?;
        let row = opened.row().clone();
        if matches!(opened, notchling_engine::consent::Opened::Fresh(_)) {
            said(AgentSetResult::Asked, &value);
        } else {
            scope::mark_recorded();
        }
        let now = consent.wait_for(&row.id, wait).unwrap_or(row);
        consent.note(&now);
        Ok(ask_view(consent, &now))
    }

    fn asked(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("settings.asked takes {id, wait?: seconds}"))?;
        let wait = args
            .get("wait")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(MAX_CALL_WAIT);
        let consent = self
            .consent
            .as_ref()
            .ok_or_else(|| ApiError::internal("settings has no consent ops"))?;
        let gone = || failed("No such ask: it may have ended over a week ago.");
        let seen = consent.row_of(caller, id).map_err(|e| match e.code {
            ErrorCode::Forbidden => e,
            _ => gone(),
        })?;
        if seen.kind != SETTING {
            return Err(gone());
        }
        let row = consent
            .wait_for(id, Duration::from_secs(wait))
            .ok_or_else(gone)?;
        Ok(ask_view(consent, &row))
    }
}

fn ask_view(consent: &ConsentOps, row: &Consent) -> Value {
    json!({"id": row.id, "key": row.target, "value": row.change.get("after"),
           "state": row.state.as_str(), "message": ask_message(row),
           "request": consent.project(row, true)})
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
        match op {
            ops::SETTINGS_SET => Some(OpSpec::AGENT_ACTS),
            op if SETTINGS_OPS.contains(&op) => Some(OpSpec::AGENT_READ),
            _ => None,
        }
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::SETTINGS_LIST => Ok(self.list(caller)),
            ops::SETTINGS_GET => self.get(caller, args),
            ops::SETTINGS_SET => self.set(caller, args),
            ops::SETTINGS_ASKED => self.asked(caller, args),
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
