use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use notchling_engine::sessions::history::{self, History, Item};
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};

use super::data::{Session, Tool};
use super::format::age;
use super::markers;
use super::sidebar::SidebarModel;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletedRow {
    pub id: String,
    pub name: String,
    pub folder: String,
    pub age: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Completed {
    pub open: bool,
    pub rows: Vec<CompletedRow>,
    pub more: bool,
}

pub const WATCH_EVERY_MS: i64 = 5_000;
pub const REFILL_PAGES: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub after: Option<String>,
    pub limit: usize,
    pub replace: bool,
    pub generation: u64,
}

pub struct Past {
    pub history: Arc<History>,
    pub items: Vec<Item>,
    pub next: Option<String>,
    pub loading: bool,
    pub refreshed_ms: i64,
    generation: u64,
    want: usize,
    got: usize,
    refills: usize,
}

impl Default for Past {
    fn default() -> Past {
        Past::new()
    }
}

impl Past {
    pub fn new() -> Past {
        Past {
            history: Arc::new(History::new()),
            items: Vec::new(),
            next: None,
            loading: false,
            refreshed_ms: 0,
            generation: 0,
            want: 0,
            got: 0,
            refills: 0,
        }
    }

    fn reload_limit(&self) -> usize {
        self.items.len().max(history::PAGE)
    }

    pub fn begin(&mut self, after: Option<String>, limit: usize, replace: bool) -> Request {
        self.generation += 1;
        self.loading = true;
        self.want = limit;
        self.got = 0;
        self.refills = 0;
        Request {
            after,
            limit,
            replace,
            generation: self.generation,
        }
    }

    pub fn open_request(&mut self) -> Request {
        let limit = self.reload_limit();
        self.begin(None, limit, true)
    }

    pub fn more_request(&mut self) -> Option<Request> {
        let after = self.next.clone()?;
        Some(self.begin(Some(after), history::PAGE, false))
    }

    pub fn watch_request(&mut self, now_ms: i64) -> Option<Request> {
        if self.loading || now_ms - self.refreshed_ms < WATCH_EVERY_MS {
            return None;
        }
        let limit = self.reload_limit();
        Some(self.begin(None, limit, true))
    }

    pub fn accepts(&self, request: &Request) -> bool {
        request.generation == self.generation
    }

    pub fn refill(
        &mut self,
        items: &[Item],
        hidden: &HashSet<String>,
        next: Option<&str>,
    ) -> Option<Request> {
        self.got += items.iter().filter(|i| !hidden.contains(&i.id)).count();
        let next = next?;
        if self.got >= self.want || self.refills + 1 >= REFILL_PAGES {
            return None;
        }
        self.refills += 1;
        self.loading = true;
        Some(Request {
            after: Some(next.to_owned()),
            limit: self.want - self.got,
            replace: false,
            generation: self.generation,
        })
    }

    pub fn replace(
        &mut self,
        mut items: Vec<Item>,
        next: Option<String>,
        now_ms: i64,
        keep: Option<&str>,
    ) {
        let kept = keep
            .filter(|id| !items.iter().any(|i| i.id == *id))
            .and_then(|id| self.items.iter().find(|i| i.id == id));
        if let Some(item) = kept {
            items.push(item.clone());
        }
        self.items = items;
        self.next = next;
        self.loading = false;
        self.refreshed_ms = now_ms;
    }

    pub fn add(&mut self, items: Vec<Item>, next: Option<String>) {
        for item in items {
            if !self.items.iter().any(|have| have.id == item.id) {
                self.items.push(item);
            }
        }
        self.next = next;
        self.loading = false;
    }

    pub fn holds(&self, session: &Session) -> bool {
        self.items
            .iter()
            .any(|item| item.id == session.agent.id && session_of(item) == *session)
    }

    pub fn sessions(&self) -> Vec<Session> {
        self.items.iter().map(session_of).collect()
    }
}

fn folder_name(folder: &str) -> String {
    Path::new(folder)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn session_of(item: &Item) -> Session {
    Session {
        agent: Agent {
            id: item.id.clone(),
            name: item.title.clone(),
            project: folder_name(&item.folder),
            cwd: item.folder.clone(),
            group: AgentGroup::Completed,
            label: markers::EXITED.to_string(),
            detail: None,
            since_ms: item.modified_ms,
            target: AgentTarget::Resume {
                session_id: item.id.clone(),
                cwd: item.folder.clone(),
            },
            suggested_reply: None,
            transcript_path: item.path.clone(),
            host: String::new(),
            headless: false,
            config_dir: item.config_dir.clone(),
            command: None,
            resume_session_id: None,
            at_prompt: false,
            node: None,
            worktree_path: None,
        },
        tool: Tool::Claude,
        transcript: item.path.as_ref().map(PathBuf::from),
    }
}

fn is_ended(s: &Session) -> bool {
    s.tool == Tool::Claude && s.agent.label == markers::EXITED
}

fn add_ids(ids: &mut HashSet<String>, s: &Session) {
    ids.insert(s.agent.id.clone());
    if let Some(id) = s.agent.session_id() {
        ids.insert(id);
    }
    if let Some(id) = &s.agent.resume_session_id {
        ids.insert(id.clone());
    }
    if let Some(stem) = s
        .transcript
        .as_deref()
        .and_then(Path::file_stem)
        .and_then(|stem| stem.to_str())
    {
        ids.insert(stem.to_owned());
    }
}

pub fn all_ids(sessions: &[Session]) -> HashSet<String> {
    let mut ids = HashSet::new();
    for s in sessions {
        add_ids(&mut ids, s);
    }
    ids
}

pub fn hidden_ids(sessions: &[Session], past: &Past) -> HashSet<String> {
    let mut ids = HashSet::new();
    for s in sessions.iter().filter(|s| !is_ended(s) || !past.holds(s)) {
        add_ids(&mut ids, s);
    }
    ids
}

pub fn with_past(mut all: Vec<Session>, stale: &[Session], past: &Past) -> Vec<Session> {
    all.retain(|s| !stale.contains(s));
    let hidden = all_ids(&all);
    for session in without_hidden(past.sessions(), &hidden) {
        if !all.iter().any(|s| s.agent.id == session.agent.id) {
            all.push(session);
        }
    }
    all
}

pub fn without_hidden(past: Vec<Session>, hidden: &HashSet<String>) -> Vec<Session> {
    past.into_iter()
        .filter(|s| !hidden.contains(&s.agent.id))
        .collect()
}

impl SidebarModel {
    pub fn with_completed(
        mut self,
        open: bool,
        sessions: &[Session],
        past: &Past,
        now_ms: i64,
    ) -> SidebarModel {
        if !open {
            self.completed = Completed::default();
            return self;
        }
        let mut ended: Vec<&Session> = sessions
            .iter()
            .filter(|s| is_ended(s) && !past.holds(s))
            .collect();
        ended.sort_by_key(|s| std::cmp::Reverse(s.agent.since_ms));
        let shown = hidden_ids(sessions, past);
        let mut rows: Vec<CompletedRow> = ended
            .into_iter()
            .map(|s| CompletedRow {
                id: s.agent.id.clone(),
                name: s.agent.name.clone(),
                folder: if s.agent.project.is_empty() {
                    folder_name(&s.agent.cwd)
                } else {
                    s.agent.project.clone()
                },
                age: age(now_ms, s.agent.since_ms),
            })
            .collect();
        rows.extend(
            past.items
                .iter()
                .filter(|item| !shown.contains(&item.id))
                .map(|item| CompletedRow {
                    id: item.id.clone(),
                    name: item.title.clone(),
                    folder: folder_name(&item.folder),
                    age: age(now_ms, item.modified_ms),
                }),
        );
        self.completed = Completed {
            open: true,
            rows,
            more: past.next.is_some(),
        };
        self
    }
}

#[cfg(test)]
#[path = "sidebar_completed_tests.rs"]
mod tests;
