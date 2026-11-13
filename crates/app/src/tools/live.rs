use std::collections::{BTreeMap, BTreeSet};

use notchling_engine::sessions::{Agent, AgentGroup};
use notchling_local_api::{topics, Event};
use serde_json::Value;

use super::requests::{RowTool, AGY_PREFIX};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolLive {
    pub rows: BTreeMap<String, Option<Agent>>,
    pub codex_running: BTreeSet<String>,
    pub codex_waiting: BTreeMap<String, usize>,
    pub agy_open: BTreeSet<String>,
    pub agy_working: BTreeSet<String>,
    pub agy_changes: BTreeMap<String, u64>,
    pub errors: BTreeMap<String, String>,
    pub named: BTreeMap<String, String>,
    pub unstarted: BTreeMap<String, String>,
    pub held: BTreeMap<String, String>,
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Follow {
    Waiting,
    Named(String),
    NotStarted(String),
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

impl ToolLive {
    pub fn apply(&mut self, event: &Event) -> bool {
        let data = &event.data;
        match event.event.as_str() {
            topics::CODEX_TURN => {
                if let Some(draft) = text(data, "draft") {
                    match (text(data, "state"), text(data, "id")) {
                        (Some("named"), Some(id)) => {
                            self.named.insert(draft.to_owned(), id.to_owned());
                        }
                        (Some("not_started"), _) => {
                            let why = text(data, "error").filter(|e| !e.is_empty());
                            self.unstarted.insert(
                                draft.to_owned(),
                                why.unwrap_or("Codex didn't start the session.").to_owned(),
                            );
                        }
                        _ => {}
                    }
                }
                let Some(id) = text(data, "id") else {
                    self.generation = self.generation.wrapping_add(1);
                    return true;
                };
                match text(data, "state") {
                    Some("started") => {
                        self.codex_running.insert(id.to_owned());
                        self.errors.remove(id);
                        if let Some(waiting) = self.codex_waiting.get_mut(id) {
                            *waiting = waiting.saturating_sub(1);
                            if *waiting == 0 {
                                self.codex_waiting.remove(id);
                            }
                        }
                    }
                    Some("queued") => {
                        let n = data.get("waiting").and_then(Value::as_u64).unwrap_or(1);
                        self.codex_waiting.insert(id.to_owned(), n.max(1) as usize);
                    }
                    Some("ended") | Some("failed") => {
                        self.codex_running.remove(id);
                        if let Some(error) = text(data, "error") {
                            self.errors.insert(id.to_owned(), error.to_owned());
                        }
                    }
                    _ => {}
                }
            }
            topics::SESSION_HELD => {
                let Some(id) = text(data, "id") else {
                    return false;
                };
                match text(data, "text").filter(|t| !t.is_empty()) {
                    Some(held) => self.held.insert(id.to_owned(), held.to_owned()),
                    None => self.held.remove(id),
                };
            }
            topics::SESSION_CHANGED => {
                let Some(id) = text(data, "id") else {
                    self.generation = self.generation.wrapping_add(1);
                    return true;
                };
                match data.get("row") {
                    Some(Value::Null) | None => {
                        self.rows.insert(id.to_owned(), None);
                    }
                    Some(row) => {
                        if let Ok(agent) = serde_json::from_value::<Agent>(row.clone()) {
                            self.rows.insert(id.to_owned(), Some(agent));
                        }
                    }
                }
                if id.starts_with(AGY_PREFIX) {
                    self.agy_changed(id, text(data, "change"), text(data, "error"));
                }
            }
            _ => return false,
        }
        self.generation = self.generation.wrapping_add(1);
        true
    }

    fn agy_changed(&mut self, id: &str, change: Option<&str>, error: Option<&str>) {
        match change {
            Some("closed") | Some("released") => {
                self.agy_open.remove(id);
                self.agy_working.remove(id);
            }
            Some("turn_started") => {
                self.agy_open.insert(id.to_owned());
                self.agy_working.insert(id.to_owned());
                self.errors.remove(id);
            }
            Some("elsewhere") => {}
            Some("turn_ended") => {
                self.agy_open.insert(id.to_owned());
                self.agy_working.remove(id);
            }
            Some(_) => {
                self.agy_open.insert(id.to_owned());
            }
            None => {}
        }
        if let Some(error) = error {
            self.errors.insert(id.to_owned(), error.to_owned());
        }
        *self.agy_changes.entry(id.to_owned()).or_default() += 1;
    }

    pub fn follow(&self, draft: &str) -> Follow {
        if let Some(row) = self.named.get(draft) {
            Follow::Named(row.clone())
        } else if let Some(why) = self.unstarted.get(draft) {
            Follow::NotStarted(why.clone())
        } else {
            Follow::Waiting
        }
    }

    pub fn is_working(&self, id: &str) -> bool {
        self.codex_running.contains(id) || self.agy_working.contains(id)
    }

    pub fn is_open(&self, id: &str) -> bool {
        self.agy_open.contains(id)
    }

    pub fn waiting(&self, id: &str) -> usize {
        self.codex_waiting.get(id).copied().unwrap_or(0)
    }

    pub fn overlay(&self, rows: &mut Vec<Agent>) {
        rows.retain(|a| {
            RowTool::of(&a.id) == RowTool::Claude || !matches!(self.rows.get(&a.id), Some(None))
        });
        for agent in rows.iter_mut() {
            if RowTool::of(&agent.id) == RowTool::Agy {
                if let Some(Some(live)) = self.rows.get(&agent.id) {
                    let transcript = agent.transcript_path.take();
                    let since = agent.since_ms;
                    *agent = live.clone();
                    agent.transcript_path = transcript;
                    agent.since_ms = since;
                }
            }
            if self.is_working(&agent.id) {
                agent.group = AgentGroup::Working;
                agent.label = "Working".to_owned();
            }
        }
        for (id, live) in &self.rows {
            if let Some(live) = live {
                if RowTool::of(id) != RowTool::Claude && !rows.iter().any(|a| &a.id == id) {
                    let mut agent = live.clone();
                    if self.is_working(id) {
                        agent.group = AgentGroup::Working;
                        agent.label = "Working".to_owned();
                    }
                    rows.push(agent);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::sessions::AgentTarget;
    use serde_json::json;

    fn event(name: &str, data: Value) -> Event {
        Event {
            event: name.to_owned(),
            data,
        }
    }

    fn agent(id: &str, group: AgentGroup, label: &str) -> Agent {
        Agent {
            id: id.to_owned(),
            name: id.to_owned(),
            project: "p".into(),
            cwd: "/w/p".into(),
            group,
            label: label.to_owned(),
            detail: None,
            since_ms: 1_000,
            target: AgentTarget::Resume {
                session_id: "s".into(),
                cwd: "/w/p".into(),
            },
            suggested_reply: None,
            transcript_path: Some("/w/p/rollout.jsonl".into()),
            host: "Codex".into(),
            headless: false,
            config_dir: None,
            command: None,
            resume_session_id: None,
        }
    }

    #[test]
    fn a_held_message_is_set_replaced_and_cleared_by_its_event() {
        let mut live = ToolLive::default();
        assert!(live.apply(&event(
            "session.held",
            json!({"id": "job-a", "text": "two"})
        )));
        assert_eq!(live.held["job-a"], "two");
        live.apply(&event(
            "session.held",
            json!({"id": "job-a", "text": "two\n\nthree"}),
        ));
        assert_eq!(live.held["job-a"], "two\n\nthree");
        live.apply(&event("session.held", json!({"id": "job-a", "text": null})));
        assert!(live.held.is_empty());
        assert!(!live.apply(&event("session.held", json!({"text": "x"}))));
    }

    #[test]
    fn codex_turns_run_queue_and_end() {
        let mut live = ToolLive::default();
        assert!(live.apply(&event(
            "codex.turn",
            json!({"id": "codex:s1", "session": "s1", "state": "started"})
        )));
        assert!(live.is_working("codex:s1"));
        live.apply(&event(
            "codex.turn",
            json!({"id": "codex:s1", "state": "queued", "waiting": 2}),
        ));
        assert_eq!(live.waiting("codex:s1"), 2);
        live.apply(&event(
            "codex.turn",
            json!({"id": "codex:s1", "state": "failed", "error": "exit 1"}),
        ));
        assert!(!live.is_working("codex:s1"));
        assert_eq!(live.errors["codex:s1"], "exit 1");
        live.apply(&event(
            "codex.turn",
            json!({"id": "codex:s1", "state": "started"}),
        ));
        assert_eq!(live.waiting("codex:s1"), 1);
        assert!(live.errors.is_empty());
        live.apply(&event(
            "codex.turn",
            json!({"id": "codex:s1", "state": "started"}),
        ));
        assert_eq!(live.waiting("codex:s1"), 0);
        let before = live.clone();
        live.apply(&event(
            "codex.turn",
            json!({"id": null, "state": "failed", "error": "no codex"}),
        ));
        assert_eq!(live.generation, before.generation + 1);
        assert_eq!(live.errors, before.errors);
        assert!(!live.apply(&event("sessions.changed", json!([]))));
    }

    #[test]
    fn a_draft_is_followed_to_the_session_the_daemon_names_late() {
        let mut live = ToolLive::default();
        live.apply(&event(
            "codex.turn",
            json!({"id": "codex:s9", "session": "s9", "state": "named", "draft": "codex-new:u1"}),
        ));
        assert_eq!(live.named["codex-new:u1"], "codex:s9");
        assert!(
            !live.is_working("codex:s9"),
            "naming is not the turn starting; the started event is"
        );
        live.apply(&event(
            "codex.turn",
            json!({"id": null, "state": "not_started", "draft": "codex-new:u2", "error": "no codex"}),
        ));
        assert_eq!(live.unstarted["codex-new:u2"], "no codex");
        assert_eq!(live.named.len(), 1);
        live.apply(&event(
            "codex.turn",
            json!({"id": null, "state": "not_started", "draft": "codex-new:u3"}),
        ));
        assert!(!live.unstarted["codex-new:u3"].is_empty());
        assert_eq!(
            live.follow("codex-new:u1"),
            Follow::Named("codex:s9".into())
        );
        assert_eq!(
            live.follow("codex-new:u2"),
            Follow::NotStarted("no codex".into())
        );
        assert_eq!(live.follow("codex-new:other"), Follow::Waiting);
    }

    #[test]
    fn antigravity_changes_say_what_the_daemon_holds() {
        let mut live = ToolLive::default();
        let row = serde_json::to_value(agent("agy:a1", AgentGroup::Working, "Working")).unwrap();
        live.apply(&event(
            "session.changed",
            json!({"id": "agy:a1", "session": "a1", "row": row, "change": "turn_started"}),
        ));
        assert!(live.is_open("agy:a1") && live.is_working("agy:a1"));
        assert_eq!(live.agy_changes["agy:a1"], 1);
        live.apply(&event(
            "session.changed",
            json!({"id": "agy:a1", "row": null, "change": "turn_ended", "error": "died"}),
        ));
        assert!(live.is_open("agy:a1") && !live.is_working("agy:a1"));
        assert_eq!(live.errors["agy:a1"], "died");
        live.apply(&event(
            "session.changed",
            json!({"id": "agy:a1", "row": null, "change": "closed"}),
        ));
        assert!(!live.is_open("agy:a1"));
        assert_eq!(live.agy_changes["agy:a1"], 3);
    }

    #[test]
    fn a_session_another_agy_holds_is_read_again_but_never_counted_open() {
        let mut live = ToolLive::default();
        live.apply(&event(
            "session.changed",
            json!({"id": "agy:a1", "row": null, "change": "elsewhere"}),
        ));
        assert!(
            !live.is_open("agy:a1"),
            "or Chat would never ask to open it"
        );
        assert_eq!(live.agy_changes["agy:a1"], 1, "but it reads the log again");
        live.apply(&event(
            "session.changed",
            json!({"id": "agy:a1", "row": null, "change": "loaded"}),
        ));
        live.apply(&event(
            "session.changed",
            json!({"id": "agy:a1", "row": null, "change": "released"}),
        ));
        assert!(
            !live.is_open("agy:a1"),
            "released: opened again, it loads all of it"
        );
    }

    #[test]
    fn the_overlay_replaces_removes_adds_and_marks_working() {
        let mut live = ToolLive::default();
        let fresh = Agent {
            name: "Edit hello".into(),
            transcript_path: None,
            ..agent("agy:a1", AgentGroup::Working, "Working")
        };
        live.apply(&event(
            "session.changed",
            json!({"id": "agy:a1", "row": serde_json::to_value(&fresh).unwrap(), "change": "loaded"}),
        ));
        live.apply(&event(
            "session.changed",
            json!({"id": "agy:new", "row": serde_json::to_value(agent("agy:new", AgentGroup::Completed, "Antigravity")).unwrap(), "change": "loaded"}),
        ));
        live.apply(&event(
            "session.changed",
            json!({"id": "codex:gone", "row": null, "deleted": true}),
        ));
        live.apply(&event(
            "codex.turn",
            json!({"id": "codex:s1", "state": "started"}),
        ));
        let mut rows = vec![
            agent("aa11bb22", AgentGroup::Completed, "Done"),
            agent("agy:a1", AgentGroup::Completed, "Antigravity"),
            agent("codex:gone", AgentGroup::Completed, "Idle"),
            agent("codex:s1", AgentGroup::Completed, "Idle"),
        ];
        live.overlay(&mut rows);
        let ids: Vec<&str> = rows.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["aa11bb22", "agy:a1", "codex:s1", "agy:new"]);
        assert_eq!(rows[1].name, "Edit hello");
        assert_eq!(rows[1].group, AgentGroup::Working);
        assert_eq!(
            rows[1].transcript_path.as_deref(),
            Some("/w/p/rollout.jsonl"),
            "the listed row's file stays"
        );
        assert_eq!(
            (rows[2].group, rows[2].label.as_str()),
            (AgentGroup::Working, "Working")
        );
        assert_eq!(rows[3].group, AgentGroup::Completed);
    }

    #[test]
    fn a_live_antigravity_row_keeps_when_it_was_last_used() {
        let mut live = ToolLive::default();
        let stamped = Agent {
            since_ms: 9_000_000,
            ..agent("agy:a1", AgentGroup::Completed, "Antigravity")
        };
        live.apply(&event(
            "session.changed",
            json!({"id": "agy:a1", "row": serde_json::to_value(&stamped).unwrap(), "change": "loaded"}),
        ));
        let mut rows = vec![agent("agy:a1", AgentGroup::Completed, "Antigravity")];
        live.overlay(&mut rows);
        assert_eq!(rows[0].since_ms, 1_000, "the file's time");
    }
}
