use super::alerts::alert_actions;
use super::kinds::{
    spec, ACCOUNT, CLUSTER, COST_PROFILE, DISPATCH, INSTALL, JOB, JOB_STOP, MACHINE, MODEL,
    SECRET_REQUEST, SESSION, SESSION_STOP, SETTING, SWITCH, UPDATE,
};
use super::model::{AnswerOn, Consent};
use crate::inbox::{Action, ActionId, Item, Kind, Place, Source, State};
use serde_json::{json, Value};

pub const TITLE_CHARS: usize = 80;
pub const DETAIL_CHARS: usize = 400;

pub fn item_id(row: &Consent, raised_ms: i64) -> String {
    format!("consent:{}:{raised_ms}", row.id)
}

pub fn consent_of(item_id: &str) -> Option<&str> {
    let rest = item_id.strip_prefix("consent:")?;
    rest.split(':').next().filter(|id| !id.is_empty())
}

fn cut(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn tool_label(tool: &str) -> &'static str {
    match tool {
        "claude" => "Claude Code",
        "codex" => "Codex",
        "antigravity" => "Antigravity",
        "shell" => "A program",
        _ => "An agent",
    }
}

fn shown(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "unset".to_string(),
        other => other.to_string(),
    }
}

fn change_title(row: &Consent) -> String {
    row.change
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or(&row.target)
        .to_string()
}

fn session_name(row: &Consent) -> String {
    let name = row.change.get("name").and_then(Value::as_str).unwrap_or("");
    let base = name.rsplit('/').find(|p| !p.is_empty()).unwrap_or(name);
    if base.is_empty() {
        "a session".to_string()
    } else {
        base.to_string()
    }
}

fn session_title(row: &Consent, who: &str) -> String {
    let name = session_name(row);
    match row.change.get("action").and_then(Value::as_str) {
        Some("start") => format!("{who} asks to start a session in {name}"),
        Some("say") => format!("{who} asks to send a message to {name}"),
        Some("stop") => format!("{who} asks to stop {name}"),
        Some("interrupt") => format!("{who} asks to interrupt {name}"),
        Some("remove") => format!("{who} asks to remove {name}"),
        Some("choose") => format!("{who} asks to answer a question of {name}"),
        Some("run") => format!("{who} asks to run a command in {name}"),
        Some("pick") if row.change.get("dismiss") == Some(&Value::Bool(true)) => {
            format!("{who} asks to dismiss a dialog in {name}")
        }
        Some("pick") => format!("{who} asks to pick an option in a dialog of {name}"),
        Some("browse") if row.change.get("on") == Some(&Value::Bool(true)) => {
            format!("{who} asks to turn Chrome on for {name}")
        }
        Some("browse") => format!("{who} asks to turn Chrome off for {name}"),
        Some("tune") => format!("{who} asks to change an option of {name}"),
        _ => format!("{who} asks for your yes"),
    }
}

fn session_detail(row: &Consent) -> String {
    match row.change.get("action").and_then(Value::as_str) {
        Some("stop" | "interrupt") => {
            "It only stops the session. Open the card on your Mac to see more.".to_string()
        }
        _ => "Open the card on your Mac to read what it would do.".to_string(),
    }
}

fn machine_name(row: &Consent) -> String {
    let name = row
        .change
        .get("machine")
        .and_then(Value::as_str)
        .unwrap_or("");
    if name.is_empty() {
        "another machine".to_string()
    } else {
        name.to_string()
    }
}

fn job_title(row: &Consent, who: &str) -> String {
    let machine = machine_name(row);
    match row.change.get("action").and_then(Value::as_str) {
        Some("dispatch") => format!("{who} asks to run a job on {machine}"),
        Some("tail") => format!("{who} asks to read a job on {machine}"),
        Some("return") => format!("{who} asks to bring back the files of a job on {machine}"),
        Some("stop") => format!("{who} asks to stop a job on {machine}"),
        _ => format!("{who} asks for your yes"),
    }
}

fn job_detail(row: &Consent) -> String {
    match row.change.get("action").and_then(Value::as_str) {
        Some("stop") => "It only stops the job. Open the card on your Mac to see more.".to_string(),
        _ => "Open the card on your Mac to read what it would do.".to_string(),
    }
}

fn cluster_title(row: &Consent, who: &str) -> String {
    let machine = machine_name(row);
    match row.change.get("action").and_then(Value::as_str) {
        Some("create") => format!("{who} asks to create a cluster"),
        Some("token") => format!("{who} asks for a join token"),
        Some("invite") => format!("{who} asks to invite a machine to the cluster"),
        Some("member") => format!("{who} asks to change {machine} in the cluster"),
        Some("remove") => format!("{who} asks to remove {machine} from the cluster"),
        Some("allow") => format!("{who} asks to change the secrets {machine} may use"),
        Some("main") => format!("{who} asks to change a main session of the cluster"),
        Some("failover") => format!("{who} asks to change the cluster's failover"),
        Some("move_home") => format!("{who} asks to make {machine} the cluster's primary"),
        Some("move_now") => format!("{who} asks to finish moving the cluster's primary"),
        _ => format!("{who} asks for your yes"),
    }
}

fn tool_word(row: &Consent) -> &'static str {
    match row.change.get("tool").and_then(Value::as_str) {
        Some("codex") => "Codex",
        Some("antigravity") => "Antigravity",
        _ => "Claude Code",
    }
}

fn account_title(row: &Consent, who: &str) -> String {
    let tool = tool_word(row);
    match row.change.get("action").and_then(Value::as_str) {
        Some("switch") => format!("{who} asks to switch your {tool} account"),
        Some("add") => format!("{who} asks to add a {tool} account"),
        Some("remove") => format!("{who} asks to remove a {tool} account"),
        Some("move_all") => format!("{who} asks to move sessions to the main login"),
        Some("move") => format!("{who} asks to move a session to another login"),
        Some("lane_create") => format!("{who} asks to open a lane"),
        Some("lane_remove") => format!("{who} asks to close a lane"),
        Some("lane_switch") => format!("{who} asks to change the account of a lane"),
        _ => format!("{who} asks for your yes"),
    }
}

pub fn title_of(row: &Consent) -> String {
    let who = tool_label(&row.tool);
    let text = match row.kind.as_str() {
        SETTING => format!("{who} asks to change {}", change_title(row)),
        SECRET_REQUEST => format!("{who} asks you to add the secret {}", row.target),
        SESSION | SESSION_STOP => session_title(row, who),
        DISPATCH | JOB | JOB_STOP => job_title(row, who),
        CLUSTER => cluster_title(row, who),
        ACCOUNT => account_title(row, who),
        MACHINE => format!("{who} asks to change a machine"),
        UPDATE => format!("{who} asks to update a tool"),
        INSTALL => format!("{who} asks to set up Notchling"),
        COST_PROFILE => format!("{who} asks to change the Cost Profile"),
        MODEL => format!("{who} asks to download a model"),
        _ => format!("{who} asks for your yes"),
    };
    cut(&text, TITLE_CHARS)
}

pub fn detail_of(row: &Consent) -> String {
    let text = match row.kind.as_str() {
        SETTING if row.change.get("text") == Some(&Value::Bool(true)) => {
            format!(
                "{} would change. The new value is on the card.",
                change_title(row)
            )
        }
        SETTING => {
            let before = row.change.get("before").map(shown).unwrap_or_default();
            let after = row.change.get("after").map(shown).unwrap_or_default();
            format!(
                "{} is now {before}. It would become {after}.",
                change_title(row)
            )
        }
        SECRET_REQUEST => {
            "Open the form on your Mac to add it. Its value never reaches the agent.".to_string()
        }
        SESSION | SESSION_STOP => session_detail(row),
        DISPATCH | JOB | JOB_STOP => job_detail(row),
        CLUSTER | ACCOUNT | MACHINE | UPDATE | INSTALL | COST_PROFILE | MODEL => {
            "Open the card on your Mac to read what it would do.".to_string()
        }
        _ => String::new(),
    };
    cut(&text, DETAIL_CHARS)
}

fn action(id: ActionId, label: &str, primary: bool) -> Action {
    Action {
        id,
        label: label.to_string(),
        primary,
    }
}

pub fn item(row: &Consent, raised_ms: i64) -> Item {
    let kind = spec(&row.kind).map_or(Kind::Consent, |k| k.item);
    let actions = match row.answer_on {
        AnswerOn::MacOrPhone => vec![
            action(ActionId::Allow, "Allow", true),
            action(ActionId::Deny, "Deny", false),
        ],
        AnswerOn::Mac => vec![
            action(ActionId::Deny, "Deny", false),
            action(ActionId::Open, "Open on Mac", true),
        ],
    };
    let mut open = json!({"consent": row.id, "kind": row.kind});
    if row.kind == SECRET_REQUEST {
        open["vault_request"] = json!(row.id);
    }
    Item {
        id: item_id(row, raised_ms),
        kind,
        title: title_of(row),
        detail: detail_of(row),
        source: Source {
            node: None,
            session: row.session.clone(),
            job: None,
        },
        created_ms: row.created_ms,
        expires_ms: Some(row.expires_ms),
        actions,
        state: State::Open,
        answer: None,
        answered_by: None,
        seen: false,
        open: Some(open),
        closed_ms: None,
        switch: (row.kind == SWITCH).then(|| row.target.clone()),
        alert_actions: alert_actions(row),
        place: row.session.clone().map(|session| Place {
            session: Some(session),
            ..Place::default()
        }),
        ..Item::default()
    }
}
