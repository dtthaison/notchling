use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use notchling_engine::transcript::conversation;
use notchling_engine::transcript::finished;
use notchling_engine::transcript::locate::predecessor;
use notchling_engine::transcript::model::{Conversation, Timestamp};
use notchling_engine::transcript::subagents::{self, Subagent};

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

pub fn chain(path: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = path.to_string();
    for _ in 0..MAX_BACK {
        let Some(before) = predecessor(&current, name) else {
            break;
        };
        out.push(before.clone());
        current = before;
    }
    out
}

pub fn subagents(
    path: &str,
    name: &str,
    finished: &HashMap<String, String>,
    now: Timestamp,
) -> Vec<Subagent> {
    let before = chain(path, name);
    if before.is_empty() {
        return subagents::list(path, finished, now);
    }
    let mut merged: HashMap<String, String> = HashMap::new();
    for earlier in before.iter().rev() {
        merged.extend(
            finished::read(earlier)
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
    }
    merged.extend(finished.iter().map(|(k, v)| (k.clone(), v.clone())));
    let mut paths = vec![path.to_string()];
    paths.extend(before);
    subagents::list_chain(&paths, &merged, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::Scratch;
    use notchling_engine::transcript::subagents::Status;
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

    fn now() -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64()
    }

    fn agent(s: &Scratch, folder: &str, id: &str, description: &str, tail: &str) {
        s.write(
            &format!("{folder}/subagents/agent-{id}.jsonl"),
            &format!("{{}}\n{tail}"),
        );
        s.write(
            &format!("{folder}/subagents/agent-{id}.meta.json"),
            &format!(r#"{{"description":"{description}","requestShape":"background"}}"#),
        );
    }

    fn notified(id: &str) -> String {
        format!(
            r#"{{"type":"queue-operation","operation":"enqueue","content":"<task-notification>\n<task-id>{id}</task-id>\n<status>completed</status>\n</task-notification>"}}"#
        ) + "\n"
    }

    fn status_of(list: &[Subagent], id: &str) -> Status {
        list.iter().find(|a| a.id == id).expect("listed").status
    }

    #[test]
    fn every_subagent_of_the_cleared_transcript_carries_over_not_only_the_running_ones() {
        let s = Scratch::new("cleared-subagents");
        s.write("p/old.jsonl", &named("worker"));
        agent(&s, "p/old", "run", "Still going", "");
        agent(
            &s,
            "p/old",
            "done",
            "Port it",
            "Report delivered to your caller\n",
        );
        tick();
        let new = s.write("p/new.jsonl", &cleared("worker"));
        let carried = subagents(&new.to_string_lossy(), "worker", &HashMap::new(), now());
        let mut ids: Vec<&str> = carried.iter().map(|a| a.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, ["done", "run"]);
        assert_eq!(status_of(&carried, "done"), Status::Completed);
        assert_eq!(status_of(&carried, "run"), Status::Running);
    }

    #[test]
    fn a_completion_noted_in_a_later_transcript_ends_an_earlier_subagent() {
        let s = Scratch::new("cleared-finished");
        s.write("p/a.jsonl", &(named("worker") + &notified("viaold")));
        agent(&s, "p/a", "viaold", "Noted in its own transcript", "");
        agent(&s, "p/a", "viacurrent", "Noted after the clears", "");
        agent(&s, "p/a", "open", "Not noted", "");
        tick();
        s.write("p/b.jsonl", &cleared("worker"));
        agent(&s, "p/b", "viamiddle", "Noted after the first clear", "");
        tick();
        let current = s.write("p/c.jsonl", &cleared("worker"));
        let mut finished = HashMap::new();
        finished.insert("viacurrent".to_string(), "completed".to_string());
        finished.insert("viamiddle".to_string(), "failed".to_string());
        let list = subagents(&current.to_string_lossy(), "worker", &finished, now());
        assert_eq!(status_of(&list, "viaold"), Status::Completed);
        assert_eq!(status_of(&list, "viacurrent"), Status::Completed);
        assert_eq!(status_of(&list, "viamiddle"), Status::Failed);
        assert_eq!(status_of(&list, "open"), Status::Running);
    }

    #[test]
    fn a_subagent_continued_in_the_new_folder_keeps_the_description_of_the_old_one() {
        let s = Scratch::new("cleared-continued-agent");
        s.write("p/old.jsonl", &named("worker"));
        agent(&s, "p/old", "go", "Port it", "");
        tick();
        let new = s.write("p/new.jsonl", &cleared("worker"));
        let continued = s.write("p/new/subagents/agent-go.jsonl", "{}\n");
        let list = subagents(&new.to_string_lossy(), "worker", &HashMap::new(), now());
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].path, continued.to_string_lossy());
        assert_eq!(list[0].description, "Port it");
        assert!(list[0].background);
    }

    #[test]
    fn without_a_clear_the_list_is_the_transcripts_own() {
        let s = Scratch::new("cleared-none");
        let lone = s.write("p/lone.jsonl", &named("worker"));
        agent(&s, "p/lone", "mine", "Mine", "");
        let list = subagents(&lone.to_string_lossy(), "worker", &HashMap::new(), now());
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].description, "Mine");
    }
}

#[cfg(test)]
#[path = "cleared_continued_tests.rs"]
mod continued_tests;
