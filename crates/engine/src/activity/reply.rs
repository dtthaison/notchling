use super::db::EventsDb;
use super::row::{ActivityRow, DbQuery};
use notchling_events::registry;
use notchling_events::spec::Group;
use serde_json::{json, Map, Value};

pub const DEFAULT_LIMIT: usize = 200;
pub const MAX_LIMIT: usize = 1000;
pub const MAX_BYTES: usize = 900 * 1024;
const DETAIL_CHARS: usize = 200;
const WIDE_FETCH: usize = 10_000;

pub const KINDS: [&str; 6] = [
    "refused",
    "failed",
    "acted",
    "setting",
    "inbox",
    "account_switch",
];

const INBOX_OPS: [&str; 4] = [
    "inbox.answer",
    "inbox.seen",
    "vault.approval.answer",
    "vault.request.cancel",
];

const SWITCH_OPS: [&str; 4] = [
    "accounts.switch",
    "accounts.move_all",
    "lanes.switch",
    "providers.switch",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    pub kinds: Vec<String>,
    pub since_ms: u64,
    pub limit: usize,
    pub groups: Vec<String>,
    pub event_kinds: Vec<String>,
    pub session: Option<String>,
    pub before_seq: Option<u64>,
}

fn names_list(
    args: &Value,
    key: &str,
    valid: impl Fn(&str) -> bool,
    what: &str,
) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    match args.get(key) {
        None | Some(Value::Null) => {}
        Some(Value::Array(list)) => {
            for item in list {
                let name = item
                    .as_str()
                    .ok_or_else(|| format!("{key} must be a list of names"))?;
                if !valid(name) {
                    return Err(format!("no {what} named {name}"));
                }
                out.push(name.to_owned());
            }
        }
        Some(_) => return Err(format!("{key} must be a list of names")),
    }
    Ok(out)
}

impl Query {
    pub fn from_args(args: &Value) -> Result<Query, String> {
        let kinds = names_list(args, "kinds", |n| KINDS.contains(&n), "activity kind")?;
        let groups = names_list(
            args,
            "groups",
            |n| Group::ALL.iter().any(|g| g.name() == n),
            "log group",
        )?;
        let event_kinds = names_list(
            args,
            "event_kinds",
            |n| registry::find(n).is_some(),
            "event kind",
        )?;
        let since_ms = match args.get("since") {
            None | Some(Value::Null) => 0,
            Some(v) => v.as_u64().ok_or("since must be a time in milliseconds")?,
        };
        let limit = match args.get("limit") {
            None | Some(Value::Null) => DEFAULT_LIMIT,
            Some(v) => v.as_u64().ok_or("limit must be a number")? as usize,
        };
        let session = match args.get("session") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_str().ok_or("session must be a text")?.to_owned()),
        };
        let before_seq = match args.get("before_seq") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_u64().ok_or("before_seq must be a number")?),
        };
        Ok(Query {
            kinds,
            since_ms,
            limit: limit.clamp(1, MAX_LIMIT),
            groups,
            event_kinds,
            session,
            before_seq,
        })
    }

    pub fn plain(limit: usize) -> Query {
        Query {
            kinds: Vec::new(),
            since_ms: 0,
            limit,
            groups: Vec::new(),
            event_kinds: Vec::new(),
            session: None,
            before_seq: None,
        }
    }

    fn db_query(&self) -> DbQuery {
        DbQuery {
            since: (self.since_ms > 0).then_some(self.since_ms as i64),
            before_seq: self.before_seq,
            kinds: self.event_kinds.clone(),
            groups: self.groups.clone(),
            session: self.session.clone(),
            limit: if self.kinds.is_empty() {
                self.limit
            } else {
                WIDE_FETCH
            },
        }
    }
}

pub fn clean(text: &str, cap: usize) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= cap {
        return one_line;
    }
    let cut: String = one_line.chars().take(cap).collect();
    format!("{cut}\u{2026}")
}

pub fn six_kind(row: &ActivityRow, f: &Value) -> &'static str {
    match row.outcome.as_str() {
        "refused" => return "refused",
        "failed" => return "failed",
        _ => {}
    }
    if row.kind == "account.switched" {
        return "account_switch";
    }
    if row.grp == "settings" || row.kind == "switch.changed" {
        return "setting";
    }
    if row.grp == "inbox" || row.kind == "phone.answered" {
        return "inbox";
    }
    if matches!(row.kind.as_str(), "op.called" | "op.failed") {
        let op = f.get("op").and_then(Value::as_str).unwrap_or("");
        if INBOX_OPS.contains(&op) {
            return "inbox";
        }
        if SWITCH_OPS.contains(&op) {
            return "account_switch";
        }
        if op.starts_with("settings.") {
            return "setting";
        }
    }
    "acted"
}

fn text(f: &Value, key: &str) -> Option<String> {
    f.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn who(row: &ActivityRow, names: &dyn Fn(&str, &str) -> Option<String>) -> String {
    match row.actor_tier.as_deref() {
        Some("app") => "app".to_owned(),
        Some("agent") => row
            .actor_exe
            .clone()
            .unwrap_or_else(|| "unknown".to_owned()),
        Some("phone") => match &row.device {
            Some(id) => names("device", id).unwrap_or_else(|| id.clone()),
            None => "phone".to_owned(),
        },
        _ => "daemon".to_owned(),
    }
}

fn result_of(row: &ActivityRow, f: &Value) -> String {
    if row.kind == "switch.changed" {
        if let Some(on) = f.get("on").and_then(Value::as_bool) {
            return if on { "on" } else { "off" }.to_owned();
        }
    }
    let result = text(f, "result");
    match (result.as_deref(), text(f, "action")) {
        (Some("taken"), Some(action)) => action,
        (Some(result), _) => result.to_owned(),
        (None, Some(action)) => action,
        (None, None) => row.outcome.clone(),
    }
}

fn detail_of(row: &ActivityRow, f: &Value) -> Option<String> {
    let list = |key: &str| -> Option<String> {
        let items = f.get(key)?.as_array()?;
        let names: Vec<&str> = items.iter().filter_map(Value::as_str).collect();
        (!names.is_empty()).then(|| names.join(", "))
    };
    let detail = match row.kind.as_str() {
        "vault.secret_used" => list("secrets"),
        "vault.secret_requested" | "vault.entry_changed" => text(f, "name"),
        "settings.agent" | "settings.changed" => text(f, "key"),
        "switch.changed" => text(f, "switch"),
        "api.refused" => match (text(f, "why"), text(f, "switch")) {
            (Some(why), Some(switch)) => Some(format!("{why} ({switch})")),
            (why, _) => why,
        },
        "api.unknown_peer" => text(f, "why"),
        _ => None,
    };
    detail
        .map(|d| clean(&d, DETAIL_CHARS))
        .filter(|d| !d.is_empty())
}

pub fn row_json(row: &ActivityRow, names: &dyn Fn(&str, &str) -> Option<String>) -> Value {
    let f = row.fields();
    let spec = registry::find(&row.kind);
    let source = if row.audit_n.is_some() {
        "audit"
    } else if row.legacy {
        "legacy"
    } else {
        "activity"
    };
    let op = text(&f, "op").unwrap_or_else(|| row.kind.clone());
    let mut refs = Map::new();
    for (name, value) in [
        ("session", &row.session),
        ("folder", &row.folder),
        ("account", &row.account),
        ("device", &row.device),
    ] {
        if let Some(value) = value {
            refs.insert(name.to_owned(), json!(value));
        }
    }
    let mut out = json!({
        "time_ms": row.ts.max(0),
        "source": source,
        "kind": six_kind(row, &f),
        "op": clean(&op, 80),
        "who": clean(&who(row, names), 60),
        "result": clean(&result_of(row, &f), 60),
        "event": row.kind,
        "group": row.grp,
        "title": spec.map_or(row.kind.as_str(), |s| s.title),
        "outcome": row.outcome,
        "refs": Value::Object(refs),
        "seq": row.seq,
        "spooled": row.spooled,
    });
    if let Some(detail) = detail_of(row, &f) {
        out["detail"] = json!(detail);
    }
    if let Some(op_id) = &row.op_id {
        out["op_id"] = json!(op_id);
    }
    if let Some(n) = row.audit_n {
        out["audit_n"] = json!(n);
    }
    out
}

pub fn reply(
    rows: &[ActivityRow],
    q: &Query,
    names: &dyn Fn(&str, &str) -> Option<String>,
) -> Value {
    let mut picked: Vec<&ActivityRow> = rows
        .iter()
        .filter(|row| {
            q.kinds.is_empty() || q.kinds.iter().any(|k| k == six_kind(row, &row.fields()))
        })
        .collect();
    picked.sort_by_key(|row| std::cmp::Reverse((row.ts, row.seq)));
    picked.truncate(q.limit);
    let mut shown: Vec<Value> = picked.iter().map(|row| row_json(row, names)).collect();
    let mut truncated = false;
    loop {
        let size = serde_json::to_vec(&shown).map(|b| b.len()).unwrap_or(0);
        if size <= MAX_BYTES || shown.is_empty() {
            return json!({"rows": shown, "truncated": truncated});
        }
        truncated = true;
        shown.truncate(shown.len() / 2);
    }
}

pub fn list(db: &EventsDb, q: &Query, names: &dyn Fn(&str, &str) -> Option<String>) -> Value {
    match db.list(&q.db_query()) {
        Ok(rows) => reply(&rows, q, names),
        Err(_) => json!({"rows": [], "truncated": false}),
    }
}
