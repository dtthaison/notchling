use std::collections::HashMap;

use serde_json::{Map, Value};

use super::data::{Session, Tool};

pub const KEY: &str = "chatClosedSessions";
const KEEP_S: f64 = 30.0 * 24.0 * 3600.0;

pub fn closable(tool: Tool) -> bool {
    matches!(tool, Tool::Codex | Tool::Antigravity)
}

pub fn is_closed(session: &Session, closed: &HashMap<String, f64>) -> bool {
    closable(session.tool)
        && closed
            .get(&session.agent.id)
            .is_some_and(|at| session.agent.since_ms as f64 / 1000.0 <= *at)
}

pub fn close(closed: &mut HashMap<String, f64>, ids: &[String], now_s: f64) {
    closed.retain(|_, at| now_s - *at < KEEP_S);
    for id in ids {
        closed.insert(id.clone(), now_s);
    }
}

pub fn ids_of(sessions: &[Session], tool: Tool) -> Vec<String> {
    sessions
        .iter()
        .filter(|s| s.tool == tool && closable(tool))
        .map(|s| s.agent.id.clone())
        .collect()
}

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
