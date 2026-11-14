use super::row::ActivityRow;
use crate::bridge::net::files::parse_iso8601;
use crate::transcript::util::parse_date;
use notchling_events::audit_line::read_head;
use notchling_events::field_ref::{Device, Ref, Secret};
use notchling_events::kinds::choices::{ItemAction, SwitchName};
use notchling_events::kinds::phone::{PhoneAnswerResult, PhoneOp};
use notchling_events::kinds::settings::SettingKey;
use notchling_events::registry;
use notchling_events::spec::Sinks;
use notchling_events::Exe;
use serde_json::{json, Map, Value};
use std::path::Path;

const REFERENCE_DATE: f64 = 978_307_200.0;
const DAY_MS: i64 = 86_400_000;
const AUDIT_DAYS: i64 = 365;
const OP_CHARS: usize = 64;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Imported {
    pub rows: Vec<ActivityRow>,
    pub skipped: u64,
}

struct Build<'a> {
    host: &'a str,
}

pub fn expires(kind: &str, ts: i64) -> i64 {
    let days = registry::find(kind).map_or(0, |s| i64::from(s.keep_days));
    ts.saturating_add(DAY_MS * if days == 0 { AUDIT_DAYS } else { days })
}

fn op_name(raw: &str) -> String {
    let word = raw.split_whitespace().next().unwrap_or("");
    let ok = !word.is_empty()
        && word.chars().count() <= OP_CHARS
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        word.to_owned()
    } else {
        "unknown".to_owned()
    }
}

fn exe_of(peer: Option<&str>) -> Option<String> {
    let path = peer?;
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    Some(Exe::named(name).as_str().to_owned())
}

fn tier_of(tier: Option<&str>) -> Option<String> {
    match tier {
        Some("app") => Some("app".to_owned()),
        Some("agent") => Some("agent".to_owned()),
        _ => None,
    }
}

impl Build<'_> {
    fn row(&self, kind: &str, ts: i64, outcome: &str, f: Value) -> ActivityRow {
        let spec = registry::find(kind);
        let mut row = ActivityRow::blank(0, ts, kind);
        row.kv = spec.map_or(1, |s| s.kv);
        row.grp = spec.map_or("diagnostics", |s| s.group.name()).to_owned();
        row.outcome = outcome.to_owned();
        row.host = self.host.to_owned();
        row.legacy = true;
        row.expires = expires(kind, ts);
        let text = |name: &str| f.get(name).and_then(Value::as_str).map(str::to_owned);
        row.session = text("session");
        row.folder = text("folder");
        row.account = text("account");
        row.device = text("device");
        row.f = f.to_string();
        row
    }

    fn actor(&self, mut row: ActivityRow, tier: Option<&str>, exe: Option<String>) -> ActivityRow {
        row.actor_tier = tier.map(str::to_owned);
        row.actor_exe = exe;
        row
    }
}

fn why_of(head: &str) -> &'static str {
    match head {
        "peer_changed" => "peer_changed",
        "needs_approval" => "needs_approval",
        _ => "forbidden",
    }
}

fn secret_names(rest: &str) -> Vec<String> {
    rest.split(',')
        .filter(|s| !s.is_empty())
        .take(16)
        .map(|s| Ref::<Secret>::new(s).as_str().to_owned())
        .collect()
}

fn switch_name(raw_op: &str) -> Option<&'static str> {
    let mut words = raw_op.split_whitespace();
    let base = words.next()?;
    let name = match base {
        "settings.set_switch" => words.next()?,
        "settings.set_acting" => "acting",
        "bridge.set" => "bridge",
        _ => return None,
    };
    Some(
        SwitchName::from_name(name)
            .unwrap_or(SwitchName::Other)
            .name(),
    )
}

fn audit_line(b: &Build, line: &str) -> Option<ActivityRow> {
    let v: Value = serde_json::from_str(line).ok()?;
    let ts = i64::try_from(v.get("time_ms")?.as_u64()?).ok()?;
    let raw_op = v.get("op")?.as_str()?;
    let raw_result = v.get("result")?.as_str()?;
    let base = op_name(raw_op);
    let (head, rest) = raw_result.split_once(':').unwrap_or((raw_result, ""));
    let tier = tier_of(v.get("tier").and_then(Value::as_str));
    let exe = exe_of(v.get("peer").and_then(Value::as_str));
    let reason = v.get("reason").and_then(Value::as_str).unwrap_or("");
    let tier_ref = tier.as_deref();

    if head == "file_said_on_ignored" {
        let f = json!({"switch": "acting", "on": false, "cause": "file_said_on_ignored"});
        return Some(b.row("switch.changed", ts, "ok", f));
    }
    if matches!(head, "peer_unknown" | "other_user") {
        let mut f = Map::new();
        f.insert("why".into(), json!(head));
        if let Some(exe) = &exe {
            f.insert("exe".into(), json!(exe));
        }
        let row = b.row("api.unknown_peer", ts, "refused", Value::Object(f));
        return Some(b.actor(row, Some("agent"), exe));
    }
    if matches!(head, "forbidden" | "peer_changed" | "needs_approval") {
        let mut f = Map::new();
        f.insert("op".into(), json!(base));
        f.insert("tier".into(), json!(tier_ref.unwrap_or("agent")));
        f.insert("why".into(), json!(why_of(head)));
        if let Some(exe) = &exe {
            f.insert("exe".into(), json!(exe));
        }
        let row = b.row("api.refused", ts, "refused", Value::Object(f));
        return Some(b.actor(row, tier_ref, exe));
    }
    if base == "vault.use" && matches!(head, "ok" | "denied") {
        let result = if head == "ok" { "given" } else { "denied" };
        let f = json!({"secrets": secret_names(rest), "result": result});
        let outcome = if head == "ok" { "ok" } else { "refused" };
        let row = b.row("vault.secret_used", ts, outcome, f);
        return Some(b.actor(row, tier_ref, exe));
    }
    if let (Some(name), "on" | "off") = (switch_name(raw_op), head) {
        let f = json!({"switch": name, "on": head == "on", "cause": "user"});
        let row = b.row("switch.changed", ts, "ok", f);
        return Some(b.actor(row, tier_ref, exe));
    }
    if base == "settings.set" && tier_ref == Some("agent") {
        let key = reason.split_once('=').map_or(reason, |(key, _)| key);
        let key = SettingKey::from_name(key.trim()).unwrap_or(SettingKey::Unknown);
        let (result, outcome) = match head {
            "ok" => ("set", "ok"),
            "denied" => ("denied", "refused"),
            _ => ("failed", "failed"),
        };
        let f = json!({"key": key.name(), "result": result});
        let row = b.row("settings.agent", ts, outcome, f);
        return Some(b.actor(row, tier_ref, exe));
    }
    let ok = matches!(head, "ok" | "on" | "off");
    let mut f = Map::new();
    f.insert("op".into(), json!(base));
    let (kind, outcome) = if ok {
        f.insert("tier".into(), json!(tier_ref.unwrap_or("app")));
        f.insert("result".into(), json!("ok"));
        ("op.called", "ok")
    } else if head == "denied" {
        f.insert("tier".into(), json!(tier_ref.unwrap_or("agent")));
        f.insert("result".into(), json!("error"));
        ("op.called", "refused")
    } else {
        f.insert("code".into(), json!("failed"));
        ("op.failed", "failed")
    };
    let row = b.row(kind, ts, outcome, Value::Object(f));
    Some(b.actor(row, tier_ref, exe))
}

pub fn import_legacy_audit(text: &str, host: &str) -> Imported {
    let build = Build { host };
    let mut out = Imported::default();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        match audit_line(&build, line) {
            Some(row) => out.rows.push(row),
            None => out.skipped += 1,
        }
    }
    out
}

fn device_ref(name: &str) -> String {
    Ref::<Device>::new(name).as_str().to_owned()
}

fn phone_line(b: &Build, line: &str) -> Option<ActivityRow> {
    let v: Value = serde_json::from_str(line).ok()?;
    let ts = parse_iso8601(v.get("time")?.as_str()?)?;
    let raw_op = v.get("op")?.as_str()?;
    let device = device_ref(v.get("device").and_then(Value::as_str).unwrap_or("phone"));
    let (base, rest) = match raw_op.split_once(':') {
        Some((base, rest)) => (base, Some(rest)),
        None => (raw_op, None),
    };
    let (kind, outcome, f) = match (base, rest) {
        ("pair", _) => (
            "phone.paired",
            "ok",
            json!({"device": device, "action": "paired"}),
        ),
        ("revoke", _) => (
            "phone.paired",
            "ok",
            json!({"device": device, "action": "revoked"}),
        ),
        ("inbox.answer", Some(rest)) => match rest.strip_prefix("refused:") {
            Some(reason) => {
                let result =
                    PhoneAnswerResult::from_name(reason).unwrap_or(PhoneAnswerResult::Other);
                (
                    "phone.answered",
                    "refused",
                    json!({"device": device, "result": result.name()}),
                )
            }
            None => {
                let action = ItemAction::from_name(rest).unwrap_or(ItemAction::Other);
                (
                    "phone.answered",
                    "ok",
                    json!({"device": device, "result": "taken", "action": action.name()}),
                )
            }
        },
        _ => {
            let name = match base {
                "sendNow" => "send_now",
                "newSession" => "new_session",
                "takeover" => "take_over",
                other => other,
            };
            let op = PhoneOp::from_name(name).unwrap_or(PhoneOp::Other);
            let mut f = Map::new();
            f.insert("device".into(), json!(device));
            f.insert("op".into(), json!(op.name()));
            if let Some(length) = v.get("length").and_then(Value::as_u64) {
                f.insert("chars".into(), json!(length));
            }
            ("phone.acted", "ok", Value::Object(f))
        }
    };
    let row = b.row(kind, ts, outcome, f);
    Some(b.actor(row, Some("phone"), None))
}

pub fn import_remote_log(text: &str, host: &str) -> Imported {
    let build = Build { host };
    let mut out = Imported::default();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        match phone_line(&build, line) {
            Some(row) => out.rows.push(row),
            None => out.skipped += 1,
        }
    }
    out
}

pub fn import_account_log(text: &str, host: &str) -> Imported {
    let build = Build { host };
    let mut out = Imported::default();
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(text) else {
        out.skipped = u64::from(!text.trim().is_empty());
        return out;
    };
    for item in &items {
        let Some(id) = item.get("id").and_then(Value::as_str) else {
            out.skipped += 1;
            continue;
        };
        let seconds = match item.get("at") {
            Some(Value::Number(n)) => n.as_f64().map(|n| n + REFERENCE_DATE),
            Some(Value::String(s)) => parse_date(s),
            _ => None,
        };
        let Some(seconds) = seconds else {
            out.skipped += 1;
            continue;
        };
        let ts = (seconds.max(0.0) * 1000.0) as i64;
        let account = notchling_events::Ref::<notchling_events::field_ref::Account>::new(id);
        let f = json!({
            "provider": "claude",
            "to": account.as_str(),
            "cause": "other",
            "result": "done",
        });
        let mut row = build.row("account.switched", ts, "ok", f);
        row.account = Some(account.as_str().to_owned());
        out.rows.push(build.actor(row, Some("app"), None));
    }
    out
}

pub fn rebuild_from_months(months: &[(u32, String)], host: &str) -> Imported {
    let mut out = Imported::default();
    for (_, text) in months {
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            match rebuilt_row(line, host) {
                Some(Some(row)) => out.rows.push(row),
                Some(None) => {}
                None => out.skipped += 1,
            }
        }
    }
    out
}

fn rebuilt_row(line: &str, host: &str) -> Option<Option<ActivityRow>> {
    let head = read_head(line)?;
    let v: Value = serde_json::from_str(line).ok()?;
    let spec = registry::find(&head.kind);
    let Some(spec) = spec.filter(|s| s.sinks.has(Sinks::ACTIVITY)) else {
        return Some(None);
    };
    let ts = v.get("ts")?.as_i64()?;
    let f = v.get("f").cloned().unwrap_or_else(|| json!({}));
    let actor = v.get("actor");
    let actor_text = |name: &str| {
        actor
            .and_then(|a| a.get(name))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let field = |name: &str| f.get(name).and_then(Value::as_str).map(str::to_owned);
    let mut row = ActivityRow::blank(v.get("seq")?.as_u64()?, ts, &head.kind);
    row.kv = v.get("kv").and_then(Value::as_u64).map_or(1, |n| n as u32);
    row.grp = spec.group.name().to_owned();
    row.outcome = v
        .get("outcome")
        .and_then(Value::as_str)
        .unwrap_or("ok")
        .to_owned();
    row.actor_tier = actor_text("tier");
    row.actor_via = actor_text("via");
    row.actor_exe = actor_text("exe");
    row.actor_session = actor_text("session");
    row.session = field("session");
    row.folder = field("folder");
    row.account = field("account");
    row.device = field("device").or_else(|| actor_text("device"));
    row.op_id = v.get("op_id").and_then(Value::as_str).map(str::to_owned);
    row.host = v
        .get("host")
        .and_then(Value::as_str)
        .unwrap_or(host)
        .to_owned();
    row.audit_n = Some(head.n);
    row.spooled = v.get("spooled").and_then(Value::as_bool).unwrap_or(false);
    row.legacy = false;
    row.f = f.to_string();
    row.expires = expires(&head.kind, ts);
    Some(Some(row))
}

pub fn assign_seq(rows: &mut [ActivityRow], first: u64) -> u64 {
    rows.sort_by_key(|r| r.ts);
    let mut next = first;
    for row in rows.iter_mut() {
        row.seq = next;
        next += 1;
    }
    next
}

pub fn read_text(path: &Path, cap: u64) -> String {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
        return String::new();
    };
    let mut bytes = Vec::new();
    let _ = file.take(cap).read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}
