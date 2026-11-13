use super::Row;
use crate::bridge::net::files::parse_iso8601;
use crate::transcript::util::parse_date;
use serde_json::Value;

const REFERENCE_DATE: f64 = 978_307_200.0;

const OP_CHARS: usize = 80;
const WHO_CHARS: usize = 60;
const RESULT_CHARS: usize = 60;
const DETAIL_CHARS: usize = 120;
const ACCOUNT_CHARS: usize = 8;

pub const KINDS: [&str; 6] = [
    "refused",
    "failed",
    "acted",
    "setting",
    "inbox",
    "account_switch",
];

const REFUSALS: [&str; 7] = [
    "forbidden",
    "needs_approval",
    "peer_changed",
    "peer_unknown",
    "other_user",
    "file_said_on_ignored",
    "denied",
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

pub fn clean(text: &str, cap: usize) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= cap {
        return one_line;
    }
    let cut: String = one_line.chars().take(cap).collect();
    format!("{cut}\u{2026}")
}

fn exe_name(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    clean(name, WHO_CHARS)
}

fn audit_kind(op: &str, result: &str) -> &'static str {
    match result {
        "ok" | "on" | "off" => {
            if op.starts_with("settings.") {
                "setting"
            } else if INBOX_OPS.contains(&op) {
                "inbox"
            } else if SWITCH_OPS.contains(&op) {
                "account_switch"
            } else {
                "acted"
            }
        }
        r if REFUSALS.contains(&r) => "refused",
        _ => "failed",
    }
}

pub fn audit_row(line: &str) -> Option<Row> {
    let v: Value = serde_json::from_str(line).ok()?;
    let time_ms = v.get("time_ms")?.as_u64()?;
    let raw_op = v.get("op")?.as_str()?;
    let raw_result = v.get("result")?.as_str()?;
    let base = raw_op.split(' ').next().unwrap_or(raw_op);
    let head = raw_result.split(':').next().unwrap_or(raw_result);
    let kind = audit_kind(base, head);
    let tier = v.get("tier").and_then(Value::as_str);
    let who = match (tier, v.get("peer").and_then(Value::as_str)) {
        (Some("app"), _) => "app".to_owned(),
        (_, Some(peer)) => exe_name(peer),
        _ => "unknown".to_owned(),
    };
    let detail = match kind {
        "refused" | "failed" => v
            .get("reason")
            .and_then(Value::as_str)
            .map(|r| clean(r, DETAIL_CHARS))
            .filter(|r| !r.is_empty()),
        _ => None,
    };
    Some(Row {
        time_ms,
        source: "audit",
        kind,
        op: clean(raw_op, OP_CHARS),
        who,
        result: clean(head, RESULT_CHARS),
        detail,
    })
}

pub fn phone_row(line: &str) -> Option<Row> {
    let v: Value = serde_json::from_str(line).ok()?;
    let time = parse_iso8601(v.get("time")?.as_str()?)?;
    let raw_op = v.get("op")?.as_str()?;
    let device = v.get("device").and_then(Value::as_str).unwrap_or("phone");
    let (base, rest) = match raw_op.split_once(':') {
        Some((base, rest)) => (base, Some(rest)),
        None => (raw_op, None),
    };
    let (kind, result) = match (base, rest) {
        ("inbox.answer", Some(rest)) => match rest.strip_prefix("refused:") {
            Some(reason) => ("refused", format!("refused: {reason}")),
            None => ("inbox", rest.to_owned()),
        },
        _ => ("acted", "ok".to_owned()),
    };
    Some(Row {
        time_ms: u64::try_from(time).unwrap_or(0),
        source: "phone",
        kind,
        op: clean(base, OP_CHARS),
        who: clean(device, WHO_CHARS),
        result: clean(&result, RESULT_CHARS),
        detail: None,
    })
}

pub fn account_rows(data: &[u8]) -> Vec<Row> {
    let Ok(Value::Array(items)) = serde_json::from_slice::<Value>(data) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?;
            let seconds = match item.get("at")? {
                Value::Number(n) => n.as_f64()? + REFERENCE_DATE,
                Value::String(s) => parse_date(s)?,
                _ => return None,
            };
            let short: String = id.chars().take(ACCOUNT_CHARS).collect();
            Some(Row {
                time_ms: (seconds.max(0.0) * 1000.0) as u64,
                source: "account",
                kind: "account_switch",
                op: "account.active".to_owned(),
                who: "app".to_owned(),
                result: "switched".to_owned(),
                detail: Some(short),
            })
        })
        .collect()
}
