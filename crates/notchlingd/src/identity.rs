use crate::caller_session::{find_session_where, is_claude_binary, LINEAGE_DEPTH};
use crate::late_emit::LateSalt;
use crate::vault_caller::Processes;
use notchling_engine::ask::delegate_tool;
use notchling_engine::ask::evidence::{command_hash, output_hash};
use notchling_engine::control::session::home_of;
use notchling_engine::organizations::Organizations;
use notchling_engine::sessions::Roots;
use notchling_events::kinds::choices::AgentTool;
use notchling_local_api::AgentIdentity;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

const ROOT_DEPTH: usize = 4096;
const UNROOTED: &str = "unrooted";

type LaunchRows = HashMap<(u32, u64), (AgentTool, String)>;

#[derive(Clone, Default)]
pub struct Launches {
    rows: Arc<Mutex<LaunchRows>>,
}

impl Launches {
    pub fn register(&self, pid: u32, started: u64, tool: AgentTool, session: &str) {
        self.rows
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert((pid, started), (tool, session.to_string()));
    }

    pub fn forget(&self, pid: u32, started: u64) {
        self.rows
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&(pid, started));
    }

    pub(crate) fn find(&self, pid: u32, started: Option<u64>) -> Option<(AgentTool, String)> {
        let rows = self.rows.lock().unwrap_or_else(PoisonError::into_inner);
        rows.get(&(pid, started?)).cloned()
    }
}

pub struct Identifier {
    roots: Roots,
    procs: Arc<dyn Processes>,
    home: PathBuf,
    sessions: Arc<Mutex<Value>>,
    organizations: Option<Organizations>,
    salt: LateSalt,
    launches: Launches,
    own_exe: PathBuf,
}

impl Identifier {
    pub fn new(
        roots: Roots,
        procs: Arc<dyn Processes>,
        sessions: Arc<Mutex<Value>>,
        salt: LateSalt,
        launches: Launches,
    ) -> Identifier {
        let home = PathBuf::from(home_of(&roots));
        Identifier {
            roots,
            procs,
            home,
            sessions,
            organizations: None,
            salt,
            launches,
            own_exe: std::env::current_exe().unwrap_or_default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn home(&self) -> &Path {
        &self.home
    }

    pub fn with_organizations(mut self, organizations: Organizations) -> Identifier {
        self.organizations = Some(organizations);
        self
    }

    pub fn with_own_exe(mut self, exe: PathBuf) -> Identifier {
        self.own_exe = exe;
        self
    }

    pub fn identify(&self, pid: u32) -> AgentIdentity {
        let chain = self.chain(pid);
        let hit = find_session_where(&self.roots, &|p| self.procs.parent(p), pid, &|p| {
            is_claude_binary(self.procs.as_ref(), p, &self.home)
        });
        let mut who = AgentIdentity::shell();
        who.root = self.root_of(pid);
        match (self.launched(&chain), self.delegated(&chain), hit) {
            (Some((tool, session)), delegated, _) => {
                who.tool = delegated.unwrap_or(tool);
                who.session = Some(session);
                who.verified = true;
            }
            (None, Some(tool), hit) => {
                who.tool = tool;
                who.verified = hit.is_some();
                who.session = hit.map(|h| h.session_id);
            }
            (None, None, Some(hit)) => {
                who.tool = AgentTool::Claude;
                who.session = Some(hit.session_id);
                who.verified = true;
            }
            (None, None, None) => who.tool = self.tool_by_name(&chain),
        }
        if let Some(session) = who.session.as_deref() {
            who.folder = self.folder_of(session);
        }
        if who.verified {
            who.organization = self.organization_of(who.folder.as_deref());
        }
        let salt = self.salt.get().map(|s| s.0);
        if let Some(salt) = salt {
            who.cmd_hash = self
                .procs
                .argv(pid)
                .and_then(|argv| command_hash(&salt, &argv));
            who.out_hash = self
                .procs
                .stdout_path(pid)
                .filter(|path| std::fs::metadata(path).is_ok_and(|m| m.is_file()))
                .map(|path| output_hash(&salt, &path));
        }
        who
    }

    fn chain(&self, pid: u32) -> Vec<u32> {
        let mut seen = HashSet::from([pid]);
        let mut chain = vec![pid];
        let mut next = self.procs.parent(pid);
        while chain.len() <= LINEAGE_DEPTH {
            let Some(p) = next.filter(|p| *p > 1 && seen.insert(*p)) else {
                break;
            };
            chain.push(p);
            next = self.procs.parent(p);
        }
        chain
    }

    fn root_of(&self, pid: u32) -> Option<String> {
        let mut seen = HashSet::from([pid]);
        let mut top = pid;
        let mut reached = false;
        for _ in 0..ROOT_DEPTH {
            match self.procs.parent(top).filter(|p| *p > 1) {
                None => {
                    reached = true;
                    break;
                }
                Some(p) if seen.insert(p) => top = p,
                Some(_) => break,
            }
        }
        if !reached {
            return Some(UNROOTED.to_owned());
        }
        let exe = self
            .procs
            .exe(top)
            .or_else(|| self.procs.name(top).map(PathBuf::from))?;
        let exe = self.procs.resolve(&exe).unwrap_or(exe);
        let started = self.procs.start_time(top).unwrap_or(0);
        Some(format!("{}:{started}", exe.to_string_lossy()))
    }

    fn launched(&self, chain: &[u32]) -> Option<(AgentTool, String)> {
        chain
            .iter()
            .find_map(|p| self.launches.find(*p, self.procs.start_time(*p)))
    }

    fn delegated(&self, chain: &[u32]) -> Option<AgentTool> {
        chain.iter().find_map(|p| {
            let exe = self.procs.exe(*p)?;
            if !self.same_file(&exe, &self.own_exe) {
                return None;
            }
            match delegate_tool(&self.procs.argv(*p)?)? {
                notchling_engine::ask::Asker::Codex => Some(AgentTool::Codex),
                notchling_engine::ask::Asker::Antigravity => Some(AgentTool::Antigravity),
                _ => None,
            }
        })
    }

    fn same_file(&self, a: &Path, b: &Path) -> bool {
        a == b
            || matches!(
                (self.procs.resolve(a), self.procs.resolve(b)),
                (Some(x), Some(y)) if x == y
            )
    }

    fn tool_by_name(&self, chain: &[u32]) -> AgentTool {
        for pid in chain.iter().skip(1) {
            let name = self.procs.name(*pid).unwrap_or_default();
            match name.as_str() {
                "claude" => return AgentTool::Claude,
                "codex" => return AgentTool::Codex,
                "agy" | "antigravity" => return AgentTool::Antigravity,
                _ => {}
            }
        }
        AgentTool::Shell
    }

    fn folder_of(&self, session: &str) -> Option<String> {
        let list = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        list.as_array()?
            .iter()
            .find(|row| row.get("id").and_then(Value::as_str) == Some(session))
            .and_then(|row| row.get("cwd").and_then(Value::as_str))
            .filter(|cwd| !cwd.is_empty())
            .map(str::to_string)
    }

    fn organization_of(&self, folder: Option<&str>) -> Option<String> {
        let found = self
            .organizations
            .as_ref()?
            .view()
            .folder_here(Path::new(folder?), &self.home);
        Some(found.organization)
    }
}

pub fn for_daemon(
    layout: &crate::layout::Layout,
    sessions: &Arc<Mutex<Value>>,
    salt: &LateSalt,
    launches: &Launches,
    names: &std::sync::OnceLock<Arc<crate::agent_named::AgentNames>>,
    events: &Arc<crate::late_emit::LateEmit>,
) -> (Arc<Identifier>, Arc<crate::agent_named::AgentNames>) {
    let home = PathBuf::from(notchling_engine::control::session::home_of(&layout.roots()));
    let names = names
        .get_or_init(|| {
            Arc::new(crate::agent_named::AgentNames::spawn(
                events.clone(),
                salt.clone(),
                sessions.clone(),
            ))
        })
        .clone();
    let identity = Identifier::new(
        layout.roots(),
        Arc::new(crate::vault_caller::SystemProcesses),
        sessions.clone(),
        salt.clone(),
        launches.clone(),
    )
    .with_organizations(Organizations::new(&layout.data_dir).with_home(&home));
    (Arc::new(identity), names)
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;
