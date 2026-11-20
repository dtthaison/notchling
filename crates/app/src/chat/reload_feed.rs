use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::session_reload::{parse_task, Task};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReloadFeed {
    slots: BTreeMap<String, (u64, Option<Task>)>,
}

impl ReloadFeed {
    pub fn seed(reply: &Value) -> ReloadFeed {
        let mut feed = ReloadFeed::default();
        for item in reply
            .get("tasks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            feed.apply(item);
        }
        feed
    }

    pub fn apply(&mut self, data: &Value) -> bool {
        let (Some(id), Some(rev)) = (
            data.get("id").and_then(Value::as_str),
            data.get("rev").and_then(Value::as_u64),
        ) else {
            return false;
        };
        if rev <= self.rev(id) {
            return false;
        }
        let task = data.get("task").and_then(parse_task);
        self.slots.insert(id.to_owned(), (rev, task));
        true
    }

    pub fn absorb(&mut self, id: &str, reply: &Value) -> bool {
        self.apply(&json!({
            "id": id,
            "rev": reply.get("rev").cloned().unwrap_or(Value::Null),
            "task": reply.get("task").cloned().unwrap_or(Value::Null),
        }))
    }

    pub fn rev(&self, id: &str) -> u64 {
        self.slots.get(id).map_or(0, |slot| slot.0)
    }

    pub fn task(&self, id: &str) -> Option<&Task> {
        self.slots.get(id).and_then(|slot| slot.1.as_ref())
    }

    pub fn live(&self, id: &str) -> bool {
        self.task(id).is_some_and(|task| !task.phase.is_over())
    }

    pub fn ended(&self) -> impl Iterator<Item = (&str, u64, &Task)> {
        self.slots.iter().filter_map(|(id, (rev, task))| {
            task.as_ref()
                .filter(|task| task.phase.is_over())
                .map(|task| (id.as_str(), *rev, task))
        })
    }
}
