use std::collections::BTreeSet;

use notchling_kit::Status;
use serde_json::Value;

use super::data::Session;
use super::sidebar::SidebarModel;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ticket {
    pub id: String,
    pub session: Option<String>,
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
    })
}

pub fn tickets(approvals: &[Value], requests: &[Value]) -> Vec<Ticket> {
    approvals
        .iter()
        .chain(requests)
        .filter_map(ticket_of)
        .collect()
}

pub fn row_for(sessions: &[Session], session: &str) -> Option<String> {
    sessions
        .iter()
        .find(|s| s.agent.id == session || s.agent.session_id().as_deref() == Some(session))
        .map(|s| s.agent.id.clone())
}

pub fn place(ticket: &Ticket, sessions: &[Session]) -> Placement {
    match ticket
        .session
        .as_deref()
        .and_then(|session| row_for(sessions, session))
    {
        Some(row) => Placement::Card(row),
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
