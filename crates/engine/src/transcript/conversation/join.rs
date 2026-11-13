use super::session_cursors;
use crate::transcript::continued;
use crate::transcript::model::{ChatItem, Conversation, ItemKind, SessionFacts};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

const MAX_EARLIER_ITEMS: usize = 1000;
const MAX_JOINED: usize = 100;

struct Joined {
    own: Arc<Conversation>,
    earlier: Vec<Arc<Conversation>>,
    result: Arc<Conversation>,
}

fn cache() -> &'static Mutex<HashMap<String, Joined>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Joined>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn empty() -> Arc<Conversation> {
    static EMPTY: OnceLock<Arc<Conversation>> = OnceLock::new();
    EMPTY.get_or_init(Arc::default).clone()
}

fn same(a: &[Arc<Conversation>], b: &[Arc<Conversation>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| Arc::ptr_eq(x, y))
}

pub(super) fn read(path: &str) -> Arc<Conversation> {
    let own = session_cursors().read(path).unwrap_or_else(empty);
    let before = continued::earlier(path);
    if before.is_empty() {
        return own;
    }
    let earlier: Vec<Arc<Conversation>> = before
        .iter()
        .map(|p| session_cursors().read(p).unwrap_or_else(empty))
        .collect();
    let known = cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(path)
        .filter(|k| Arc::ptr_eq(&k.own, &own) && same(&k.earlier, &earlier))
        .map(|k| k.result.clone());
    if let Some(result) = known {
        return result;
    }
    let result = Arc::new(join(&own, &before, &earlier));
    let mut joined = cache().lock().unwrap_or_else(|e| e.into_inner());
    if joined.len() >= MAX_JOINED && !joined.contains_key(path) {
        joined.clear();
    }
    joined.insert(
        path.to_string(),
        Joined {
            own,
            earlier,
            result: result.clone(),
        },
    );
    result
}

fn join(own: &Conversation, paths: &[String], earlier: &[Arc<Conversation>]) -> Conversation {
    let mut items: Vec<ChatItem> = Vec::new();
    for (path, before) in paths.iter().zip(earlier) {
        let from = Path::new(path)
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        items.extend(before.items.iter().map(|item| earlier_item(&from, item)));
    }
    if items.len() > MAX_EARLIER_ITEMS {
        items.drain(..items.len() - MAX_EARLIER_ITEMS);
    }
    items.extend(own.items.iter().cloned());
    let mut joined = own.clone();
    joined.items = items;
    joined.facts = facts(&own.facts, earlier);
    let mut finished = HashMap::new();
    for before in earlier {
        finished.extend(before.finished_agents.clone());
    }
    finished.extend(own.finished_agents.clone());
    joined.finished_agents = finished;
    joined
}

fn earlier_item(from: &str, item: &ChatItem) -> ChatItem {
    let mut item = item.clone();
    item.id = format!("{from}:{}", item.id);
    if let ItemKind::Tools(calls) = &mut item.kind {
        for call in calls {
            call.finished = true;
        }
    }
    item
}

fn facts(own: &SessionFacts, earlier: &[Arc<Conversation>]) -> SessionFacts {
    let mut facts = own.clone();
    let mut files: Vec<String> = Vec::new();
    for before in earlier {
        let f = &before.facts;
        facts.input_tokens = facts.input_tokens.saturating_add(f.input_tokens);
        facts.output_tokens = facts.output_tokens.saturating_add(f.output_tokens);
        facts.cached_tokens = facts.cached_tokens.saturating_add(f.cached_tokens);
        facts.turns = facts.turns.saturating_add(f.turns);
        facts.commands = facts.commands.saturating_add(f.commands);
        for file in &f.files {
            if !files.contains(file) {
                files.push(file.clone());
            }
        }
    }
    for file in &own.files {
        if !files.contains(file) {
            files.push(file.clone());
        }
    }
    facts.files = files;
    for before in earlier.iter().rev() {
        let f = &before.facts;
        if facts.model.is_none() {
            facts.model.clone_from(&f.model);
        }
        if facts.effort.is_none() {
            facts.effort.clone_from(&f.effort);
        }
        if facts.branch.is_none() {
            facts.branch.clone_from(&f.branch);
        }
        if facts.context_tokens == 0 {
            facts.context_tokens = f.context_tokens;
        }
    }
    facts
}
