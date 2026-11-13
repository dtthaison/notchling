use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use notchling_engine::transcript::conversation;
use notchling_engine::transcript::locate::predecessor;
use notchling_engine::transcript::model::{Conversation, Timestamp};
use notchling_engine::transcript::subagents::{self, Status, Subagent};

const MAX_BACK: usize = 4;

fn earlier_model(path: &str, name: &str) -> Option<(String, Option<String>)> {
    let mut current = path.to_string();
    for _ in 0..MAX_BACK {
        let before = predecessor(&current, name)?;
        let read = conversation::read_shared(&before);
        if let Some(model) = &read.facts.model {
            return Some((model.clone(), read.facts.effort.clone()));
        }
        current = before;
    }
    None
}

type Carried = HashMap<String, (Arc<Conversation>, Option<String>, Arc<Conversation>)>;

fn carried_cache() -> &'static Mutex<Carried> {
    static CACHE: OnceLock<Mutex<Carried>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn with_model(path: &str, name: &str, read: Arc<Conversation>) -> Arc<Conversation> {
    if read.facts.model.is_some() {
        return read;
    }
    let Some((model, effort)) = earlier_model(path, name) else {
        return read;
    };
    let mut cache = carried_cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some((source, cached_model, result)) = cache.get(path) {
        if Arc::ptr_eq(source, &read) && cached_model.as_deref() == Some(model.as_str()) {
            return result.clone();
        }
    }
    let mut copy = (*read).clone();
    copy.facts.model = Some(model.clone());
    if copy.facts.effort.is_none() {
        copy.facts.effort = effort;
    }
    let result = Arc::new(copy);
    if cache.len() > 200 {
        cache.clear();
    }
    cache.insert(path.to_string(), (read, Some(model), result.clone()));
    result
}

pub fn running_before(path: &str, name: &str, now: Timestamp) -> Vec<Subagent> {
    let Some(before) = predecessor(path, name) else {
        return Vec::new();
    };
    let read = conversation::read_shared(&before);
    subagents::list(&before, &read.finished_agents, now)
        .into_iter()
        .filter(|a| a.status == Status::Running)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::Scratch;
    use std::time::Duration;

    fn tick() {
        std::thread::sleep(Duration::from_millis(50));
    }

    fn named(agent: &str) -> String {
        format!(r#"{{"type":"agent-name","agentName":"{agent}"}}"#) + "\n"
    }

    fn cleared(agent: &str) -> String {
        format!(r#"{{"type":"system","agentName":"{agent}","content":"SessionStart:clear"}}"#)
            + "\n"
    }

    fn reply(model: &str) -> String {
        format!(
            r#"{{"type":"assistant","uuid":"a1","timestamp":"2026-11-12T10:00:00Z","message":{{"id":"m1","role":"assistant","model":"{model}","content":[{{"type":"text","text":"hi"}}]}}}}"#
        ) + "\n"
    }

    #[test]
    fn a_cleared_session_keeps_the_model_it_ran_with() {
        let s = Scratch::new("cleared-model");
        s.write(
            "p/old.jsonl",
            &(named("worker") + &reply("claude-opus-5-5")),
        );
        tick();
        let new = s.write("p/new.jsonl", &cleared("worker"));
        let path = new.to_string_lossy().into_owned();
        let read = conversation::read_shared(&path);
        assert_eq!(read.facts.model, None, "the new transcript names no model");
        let shown = with_model(&path, "worker", read.clone());
        assert_eq!(shown.facts.model.as_deref(), Some("claude-opus-5-5"));
        assert!(Arc::ptr_eq(&shown, &with_model(&path, "worker", read)));
    }

    #[test]
    fn a_session_with_its_own_model_or_no_clear_is_left_alone() {
        let s = Scratch::new("cleared-own");
        s.write(
            "p/old.jsonl",
            &(named("worker") + &reply("claude-opus-5-5")),
        );
        tick();
        let new = s.write(
            "p/new.jsonl",
            &(cleared("worker") + &reply("claude-sonnet-5-5")),
        );
        let path = new.to_string_lossy().into_owned();
        let read = conversation::read_shared(&path);
        let shown = with_model(&path, "worker", read.clone());
        assert!(Arc::ptr_eq(&shown, &read));
        assert_eq!(shown.facts.model.as_deref(), Some("claude-sonnet-5-5"));

        let lone = s.write("q/lone.jsonl", &named("worker"));
        let lone = lone.to_string_lossy().into_owned();
        let read = conversation::read_shared(&lone);
        assert_eq!(with_model(&lone, "worker", read).facts.model, None);
    }

    #[test]
    fn running_subagents_of_the_cleared_transcript_carry_over() {
        let s = Scratch::new("cleared-subagents");
        s.write("p/old.jsonl", &named("worker"));
        s.write("p/old/subagents/agent-a1.jsonl", "{}\n");
        s.write(
            "p/old/subagents/agent-a1.meta.json",
            r#"{"description":"Port it","requestShape":"background"}"#,
        );
        tick();
        let new = s.write("p/new.jsonl", &cleared("worker"));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        let carried = running_before(&new.to_string_lossy(), "worker", now);
        assert_eq!(carried.len(), 1);
        assert_eq!(carried[0].description, "Port it");
    }
}
