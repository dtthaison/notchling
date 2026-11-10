//! Codex and Antigravity rows closed in Chat (`chatClosedSessions`: row id to when, in seconds, the
//! Swift app's key). Closing only takes the row off the sidebar: the session stays in Codex's or
//! Antigravity's own files, and the row comes back once the session is used again. Claude Code
//! sessions aren't closed this way; they leave the sidebar when they exit.

use std::collections::HashMap;

use serde_json::{Map, Value};

use super::data::{Session, Tool};

/// The setting's key.
pub const KEY: &str = "chatClosedSessions";
/// How long a closed row is remembered, in seconds (the rows themselves go after 14 days).
const KEEP_S: f64 = 30.0 * 24.0 * 3600.0;

/// Whether `tool`'s rows can be closed.
pub fn closable(tool: Tool) -> bool {
    matches!(tool, Tool::Codex | Tool::Antigravity)
}

/// Whether `session` was closed after it was last used.
pub fn is_closed(session: &Session, closed: &HashMap<String, f64>) -> bool {
    closable(session.tool)
        && closed
            .get(&session.agent.id)
            .is_some_and(|at| session.agent.since_ms as f64 / 1000.0 <= *at)
}

/// Closes `ids` at `now_s`, forgetting closes older than a month.
pub fn close(closed: &mut HashMap<String, f64>, ids: &[String], now_s: f64) {
    closed.retain(|_, at| now_s - *at < KEEP_S);
    for id in ids {
        closed.insert(id.clone(), now_s);
    }
}

/// The ids of every `tool` row in `sessions`, for Close All.
pub fn ids_of(sessions: &[Session], tool: Tool) -> Vec<String> {
    sessions
        .iter()
        .filter(|s| s.tool == tool && closable(tool))
        .map(|s| s.agent.id.clone())
        .collect()
}

/// The setting's value read back (anything that isn't a number is skipped).
pub fn from_value(value: Option<&Value>) -> HashMap<String, f64> {
    value
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| v.as_f64().map(|at| (k.clone(), at)))
                .collect()
        })
        .unwrap_or_default()
}

pub fn to_value(closed: &HashMap<String, f64>) -> Value {
    let mut map = Map::new();
    for (id, at) in closed {
        map.insert(id.clone(), Value::from(*at));
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::sidebar::tests::session;
    use notchling_engine::sessions::AgentGroup;

    fn row(id: &str, tool: Tool, since_ms: i64) -> Session {
        let mut s = session(id, AgentGroup::Completed, None, tool);
        s.agent.since_ms = since_ms;
        s
    }

    #[test]
    fn a_closed_row_stays_hidden_until_it_is_used_again() {
        let mut closed = HashMap::new();
        close(&mut closed, &["codex:a".into()], 100.0);
        assert!(is_closed(&row("codex:a", Tool::Codex, 90_000), &closed));
        assert!(
            !is_closed(&row("codex:a", Tool::Codex, 101_000), &closed),
            "used after"
        );
        assert!(!is_closed(&row("codex:b", Tool::Codex, 90_000), &closed));
    }

    #[test]
    fn claude_code_rows_are_never_closed_here() {
        let mut closed = HashMap::new();
        close(&mut closed, &["abc".into()], 100.0);
        assert!(!is_closed(&row("abc", Tool::Claude, 1), &closed));
    }

    #[test]
    fn close_all_takes_one_tool_and_old_closes_are_forgotten() {
        let rows = vec![
            row("codex:a", Tool::Codex, 1),
            row("agy:b", Tool::Antigravity, 1),
            row("codex:c", Tool::Codex, 1),
            row("x", Tool::Claude, 1),
        ];
        assert_eq!(ids_of(&rows, Tool::Codex), ["codex:a", "codex:c"]);
        assert!(ids_of(&rows, Tool::Claude).is_empty());
        let mut closed = HashMap::from([("agy:old".to_owned(), 0.0)]);
        close(&mut closed, &ids_of(&rows, Tool::Antigravity), KEEP_S + 1.0);
        assert!(!closed.contains_key("agy:old"));
        assert!(closed.contains_key("agy:b"));
    }

    #[test]
    fn the_setting_reads_back_what_was_written() {
        let closed = HashMap::from([("codex:a".to_owned(), 12.5)]);
        assert_eq!(from_value(Some(&to_value(&closed))), closed);
        assert!(from_value(Some(&Value::from("junk"))).is_empty());
        assert!(from_value(None).is_empty());
    }
}
