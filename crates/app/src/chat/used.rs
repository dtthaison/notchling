//! Codex and Antigravity sessions used in Chat (`chatUsedSessions`: row id to when, in seconds).
//! Most of these tools' sessions are made elsewhere: agents hand jobs over with `--delegate`, and
//! the tools' own terminals make more. So the Sidebar shows one only once it was used here (opened,
//! started, sent to); Search finds every one, and opening it from there counts as using it.
//! Claude Code sessions always show.

use std::collections::HashMap;

use super::closed::closable;
use super::data::Session;

/// The setting's key.
pub const KEY: &str = "chatUsedSessions";
/// How long a use is remembered, in seconds (the rows themselves go after 14 days).
const KEEP_S: f64 = 30.0 * 24.0 * 3600.0;

/// A draft (`codex-new:`, `agy-new:`): being started here.
pub fn is_draft(id: &str) -> bool {
    id.starts_with("codex-new:") || id.starts_with("agy-new:")
}

/// Whether the Sidebar shows `session`: any Claude Code row, a draft, or one used here.
pub fn shown(session: &Session, used: &HashMap<String, f64>) -> bool {
    !closable(session.tool) || is_draft(&session.agent.id) || used.contains_key(&session.agent.id)
}

/// Records a use of `id` at `now_s`, forgetting uses older than a month. False when nothing
/// changed that needs saving (a Claude Code row, a draft, or used already today).
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

/// Forgets `ids` (closed in Chat).
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
    use notchling_engine::sessions::AgentGroup;

    fn row(id: &str, tool: Tool) -> Session {
        session(id, AgentGroup::Completed, None, tool)
    }

    #[test]
    fn only_rows_used_here_show_and_claude_code_always_does() {
        let mut used = HashMap::new();
        assert!(shown(&row("abc", Tool::Claude), &used));
        assert!(!shown(&row("codex:a", Tool::Codex), &used));
        assert!(!shown(&row("agy:b", Tool::Antigravity), &used));
        assert!(shown(&row("codex-new:1", Tool::Codex), &used), "a draft");
        assert!(mark(&mut used, "codex:a", 100.0));
        assert!(shown(&row("codex:a", Tool::Codex), &used));
        assert!(!mark(&mut used, "codex:a", 200.0), "already today");
        assert!(!mark(&mut used, "abc", 200.0));
        assert!(!mark(&mut used, "agy-new:1", 200.0));
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
