//! What the daemon's `tools` topic says about Codex and Antigravity sessions right now, kept by
//! the session source and read by Chat and the notch: a Codex turn running (`codex.turn`), a
//! message waiting behind it, an Antigravity session the daemon holds open and whether its turn
//! runs (`session.changed`), and the row the daemon describes for a session the files don't list
//! yet (one started this run) or no longer should (one deleted).
//!
//! The files stay the source of the rows themselves; this lays the live state over them
//! ([`ToolLive::overlay`]), the way `NotchForeign.merge` did over Chat's listing.

use std::collections::{BTreeMap, BTreeSet};

use notchling_engine::sessions::{Agent, AgentGroup};
use notchling_local_api::{topics, Event};
use serde_json::Value;

use super::requests::{RowTool, AGY_PREFIX};

/// The live state, from the events so far.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolLive {
    /// Rows the daemon described, by row id: the row as it is now, or `None` once it is gone.
    pub rows: BTreeMap<String, Option<Agent>>,
    /// Codex sessions with a turn running.
    pub codex_running: BTreeSet<String>,
    /// How many messages wait behind each Codex session's running turn.
    pub codex_waiting: BTreeMap<String, usize>,
    /// Antigravity sessions the daemon holds open, and those with a turn running.
    pub agy_open: BTreeSet<String>,
    pub agy_working: BTreeSet<String>,
    /// Counts each change to an Antigravity session's conversation, so a reader knows to ask for
    /// it again.
    pub agy_changes: BTreeMap<String, u64>,
    /// The last failure of a session's turn, cleared when its next turn starts.
    pub errors: BTreeMap<String, String>,
    /// Draft rows (`codex-new:<uuid>`) whose session Codex named after the daemon had replied
    /// `starting`: the draft, and the real row it became.
    pub named: BTreeMap<String, String>,
    /// Draft rows whose session never started, and why.
    pub unstarted: BTreeMap<String, String>,
    /// Counts every event taken, so a poll can tell something moved.
    pub generation: u64,
}

/// What became of a draft row whose session was still starting when the daemon replied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Follow {
    Waiting,
    /// Codex named the session: this is its row.
    Named(String),
    NotStarted(String),
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

impl ToolLive {
    /// Takes one event. True when it was one of the `tools` topic's.
    pub fn apply(&mut self, event: &Event) -> bool {
        let data = &event.data;
        match event.event.as_str() {
            topics::CODEX_TURN => {
                // A new session the daemon could not name within its reply: the draft row it
                // was sent from, and what became of it.
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
            Some("closed") => {
                self.agy_open.remove(id);
                self.agy_working.remove(id);
            }
            Some("turn_started") => {
                self.agy_open.insert(id.to_owned());
                self.agy_working.insert(id.to_owned());
                self.errors.remove(id);
            }
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

    /// What became of a draft whose first message the daemon answered `starting`.
    pub fn follow(&self, draft: &str) -> Follow {
        if let Some(row) = self.named.get(draft) {
            Follow::Named(row.clone())
        } else if let Some(why) = self.unstarted.get(draft) {
            Follow::NotStarted(why.clone())
        } else {
            Follow::Waiting
        }
    }

    /// Whether a row's turn runs, by the runners' word.
    pub fn is_working(&self, id: &str) -> bool {
        self.codex_running.contains(id) || self.agy_working.contains(id)
    }

    /// Whether the daemon holds an Antigravity session open (its conversation can be read).
    pub fn is_open(&self, id: &str) -> bool {
        self.agy_open.contains(id)
    }

    /// How many messages wait behind a Codex session's turn.
    pub fn waiting(&self, id: &str) -> usize {
        self.codex_waiting.get(id).copied().unwrap_or(0)
    }

    /// Lays the live state over rows read from the files: a session the daemon says is gone is
    /// dropped; an Antigravity row the daemon describes takes its name and state (the summaries
    /// copy lags behind); one the files don't list yet is added; and a running turn makes its row
    /// Working whatever the files say. Claude Code rows are left alone.
    pub fn overlay(&self, rows: &mut Vec<Agent>) {
        rows.retain(|a| {
            RowTool::of(&a.id) == RowTool::Claude || !matches!(self.rows.get(&a.id), Some(None))
        });
        for agent in rows.iter_mut() {
            if RowTool::of(&agent.id) == RowTool::Agy {
                if let Some(Some(live)) = self.rows.get(&agent.id) {
                    let transcript = agent.transcript_path.take();
                    // The daemon's row carries its event's time; when the session was last used
                    // is the file's (a closed row would reopen on every event otherwise).
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
        // The next queued turn starts: one fewer waits, and the error is gone.
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
        // A turn that never got a session (a failed draft) changes nothing but the count.
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
        // A failure with no error text still says something.
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
        // The daemon stamps its row with the time of its event; opening or reading a session
        // isn't using it, so a closed row stays closed.
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
