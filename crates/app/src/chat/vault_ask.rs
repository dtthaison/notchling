use std::collections::BTreeSet;

use notchling_kit::Status;
use serde_json::Value;

use super::data::Session;
use super::sidebar::SidebarModel;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ticket {
    pub id: String,
    pub session: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Placement {
    Card(String),
    Sheet,
}

fn ticket_of(data: &Value) -> Option<Ticket> {
    let id = data.get("id")?.as_str()?;
    (!id.is_empty()).then(|| Ticket {
        id: id.to_owned(),
        session: data
            .get("session")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        cwd: data
            .get("verified")
            .and_then(|v| v.get("cwd"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
    })
}

pub fn tickets(approvals: &[Value], requests: &[Value]) -> Vec<Ticket> {
    approvals
        .iter()
        .chain(requests)
        .filter_map(ticket_of)
        .collect()
}

fn matched<'a>(sessions: &'a [Session], session: &str) -> Option<&'a Session> {
    let mut rows = sessions.iter().filter(|s| {
        s.agent.session_id().as_deref() == Some(session)
            || s.transcript
                .as_deref()
                .and_then(|p| p.file_stem())
                .and_then(|stem| stem.to_str())
                == Some(session)
    });
    let first = rows.next()?;
    rows.next().is_none().then_some(first)
}

pub fn row_for(sessions: &[Session], session: &str) -> Option<String> {
    matched(sessions, session).map(|s| s.agent.id.clone())
}

fn real(path: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(path);
    for alias in ["tmp", "var", "etc"] {
        if let Ok(rest) = path.strip_prefix(format!("/private/{alias}")) {
            return std::path::Path::new("/").join(alias).join(rest);
        }
    }
    path.to_path_buf()
}

pub fn cwd_within(folder: &str, cwd: &str) -> bool {
    if folder.is_empty() || cwd.is_empty() {
        return false;
    }
    let (folder, cwd) = (real(folder), real(cwd));
    folder.is_absolute() && cwd.is_absolute() && cwd.starts_with(folder)
}

fn lives_there(session: &Session, cwd: Option<&str>) -> bool {
    let Some(cwd) = cwd else {
        return false;
    };
    cwd_within(&session.agent.cwd, cwd)
        || session
            .agent
            .worktree_path
            .as_deref()
            .is_some_and(|w| cwd_within(w, cwd))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Covers {
    pub active: bool,
    pub sheet: bool,
    pub inbox: bool,
    pub phone: bool,
    pub subagent: bool,
    pub terminal: bool,
    pub vault_sheet: bool,
}

pub fn visible(
    placement: &Placement,
    selected: Option<&str>,
    front: bool,
    covers: &Covers,
) -> bool {
    if !front || !covers.active || covers.inbox || covers.phone {
        return false;
    }
    match placement {
        Placement::Sheet => true,
        Placement::Card(row) => {
            selected == Some(row.as_str())
                && !covers.sheet
                && !covers.subagent
                && !covers.terminal
                && !covers.vault_sheet
        }
    }
}

pub fn pick(ids: &[&str], raised: Option<&str>) -> Option<usize> {
    if ids.is_empty() {
        return None;
    }
    Some(
        raised
            .and_then(|r| ids.iter().position(|id| *id == r))
            .unwrap_or(0),
    )
}

pub fn place(ticket: &Ticket, sessions: &[Session]) -> Placement {
    let row = ticket
        .session
        .as_deref()
        .and_then(|session| matched(sessions, session))
        .filter(|s| lives_there(s, ticket.cwd.as_deref()));
    match row {
        Some(s) => Placement::Card(s.agent.id.clone()),
        None => Placement::Sheet,
    }
}

pub fn waiting_rows(tickets: &[Ticket], sessions: &[Session]) -> BTreeSet<String> {
    tickets
        .iter()
        .filter_map(|t| match place(t, sessions) {
            Placement::Card(row) => Some(row),
            Placement::Sheet => None,
        })
        .collect()
}

impl SidebarModel {
    pub fn with_vault(mut self, waiting: &BTreeSet<String>) -> SidebarModel {
        for row in self.sections.iter_mut().flat_map(|s| &mut s.rows) {
            if waiting.contains(&row.id) {
                row.status = Status::Needs;
            }
        }
        self
    }
}

#[cfg(test)]
#[path = "vault_ask_tests.rs"]
mod tests;
