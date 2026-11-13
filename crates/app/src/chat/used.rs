use std::collections::HashMap;

use notchling_engine::sessions::AgentGroup;

use super::closed::closable;
use super::data::Session;

pub const KEY: &str = "chatUsedSessions";
const KEEP_S: f64 = 30.0 * 24.0 * 3600.0;
pub const RECENT_S: f64 = 3600.0;

pub fn is_draft(id: &str) -> bool {
    id.starts_with("codex-new:") || id.starts_with("agy-new:")
}

pub fn shown(session: &Session, used: &HashMap<String, f64>, now_s: f64) -> bool {
    let agent = &session.agent;
    !closable(session.tool)
        || is_draft(&agent.id)
        || used.contains_key(&agent.id)
        || agent.group != AgentGroup::Completed
        || now_s - agent.since_ms as f64 / 1000.0 < RECENT_S
}

pub fn is_delegated_job(
    id: &str,
    delegated: &std::collections::HashSet<String>,
    used: &HashMap<String, f64>,
) -> bool {
    delegated.contains(id) && !used.contains_key(id)
}

pub fn mark(used: &mut HashMap<String, f64>, id: &str, now_s: f64) -> bool {
    let foreign = id.starts_with("codex:") || id.starts_with("agy:");
    if !foreign {
        return false;
    }
    if used.get(id).is_some_and(|at| now_s - at < 24.0 * 3600.0) {
        return false;
    }
    used.retain(|_, at| now_s - *at < KEEP_S);
    used.insert(id.to_owned(), now_s);
    true
}

pub fn forget(used: &mut HashMap<String, f64>, ids: &[String]) -> bool {
    let before = used.len();
    used.retain(|id, _| !ids.contains(id));
    used.len() != before
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::data::Tool;
    use crate::chat::sidebar::tests::session;

    fn row(id: &str, tool: Tool) -> Session {
        session(id, AgentGroup::Completed, None, tool)
    }

    fn shown_now(session: &Session, used: &HashMap<String, f64>) -> bool {
        shown(
            session,
            used,
            session.agent.since_ms as f64 / 1000.0 + 2.0 * RECENT_S,
        )
    }

    #[test]
    fn a_hand_off_shows_while_it_works_and_an_hour_after() {
        let used = HashMap::new();
        let now = 10_000_000.0;
        let mut working = row("codex:d", Tool::Codex);
        working.agent.group = AgentGroup::Working;
        assert!(shown(&working, &used, now), "working now, made elsewhere");
        let mut asks = row("agy:d", Tool::Antigravity);
        asks.agent.group = AgentGroup::NeedsInput;
        assert!(shown(&asks, &used, now), "waiting on an answer");
        let mut done = row("codex:e", Tool::Codex);
        done.agent.since_ms = ((now - RECENT_S + 60.0) * 1000.0) as i64;
        assert!(shown(&done, &used, now), "ended within the hour");
        done.agent.since_ms = ((now - RECENT_S - 60.0) * 1000.0) as i64;
        assert!(
            !shown(&done, &used, now),
            "ended over an hour ago, never used here"
        );
    }

    #[test]
    fn only_rows_used_here_show_and_claude_code_always_does() {
        let mut used = HashMap::new();
        assert!(shown_now(&row("abc", Tool::Claude), &used));
        assert!(!shown_now(&row("codex:a", Tool::Codex), &used));
        assert!(!shown_now(&row("agy:b", Tool::Antigravity), &used));
        assert!(
            shown_now(&row("codex-new:1", Tool::Codex), &used),
            "a draft"
        );
        assert!(mark(&mut used, "codex:a", 100.0));
        assert!(shown_now(&row("codex:a", Tool::Codex), &used));
        assert!(!mark(&mut used, "codex:a", 200.0), "already today");
        assert!(!mark(&mut used, "abc", 200.0));
        assert!(!mark(&mut used, "agy-new:1", 200.0));
    }

    #[test]
    fn a_delegated_job_is_left_out_until_it_is_used_here() {
        let delegated = std::collections::HashSet::from(["codex:job".to_owned()]);
        let mut used = HashMap::new();
        assert!(is_delegated_job("codex:job", &delegated, &used));
        assert!(!is_delegated_job("codex:other", &delegated, &used));
        assert!(!is_delegated_job("abc", &delegated, &used));
        assert!(mark(&mut used, "codex:job", 100.0));
        assert!(!is_delegated_job("codex:job", &delegated, &used));
    }

    #[test]
    fn old_uses_are_forgotten_and_closing_forgets() {
        let mut used = HashMap::from([("agy:old".to_owned(), 0.0)]);
        assert!(mark(&mut used, "agy:b", KEEP_S + 1.0));
        assert!(!used.contains_key("agy:old"));
        assert!(forget(&mut used, &["agy:b".to_owned()]));
        assert!(used.is_empty());
        assert!(!forget(&mut used, &["agy:b".to_owned()]));
    }
}
