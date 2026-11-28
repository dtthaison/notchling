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

#[derive(PartialEq, Eq)]
enum CwdRoot {
    Unix,
    Drive(u8),
    Unc(String, String),
}

fn strip_verbatim(raw: &str) -> std::borrow::Cow<'_, str> {
    if raw.len() >= 8 && raw[..8].eq_ignore_ascii_case(r"\\?\UNC\") {
        return std::borrow::Cow::Owned(format!(r"\\{}", &raw[8..]));
    }
    if let Some(after) = raw.strip_prefix(r"\\?\") {
        let b = after.as_bytes();
        if b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && (b[2] == b'\\' || b[2] == b'/')
        {
            return std::borrow::Cow::Borrowed(after);
        }
    }
    std::borrow::Cow::Borrowed(raw)
}

fn parse_cwd(raw: &str) -> Option<(CwdRoot, Vec<String>)> {
    if raw.is_empty() {
        return None;
    }
    let s = strip_verbatim(raw);
    let b = s.as_bytes();
    let (root, rest, win_sep) = if let Some(after) = s.strip_prefix("\\\\") {
        let sep = ['\\', '/'];
        let i = after.find(sep)?;
        let server = &after[..i];
        if server.is_empty() || server == "." || server == ".." {
            return None;
        }
        let tail = &after[i + 1..];
        if tail.starts_with(sep) {
            return None;
        }
        let (share, rest) = match tail.find(sep) {
            Some(j) => (&tail[..j], &tail[j + 1..]),
            None => (tail, ""),
        };
        if share.is_empty() || share == "." || share == ".." {
            return None;
        }
        (
            CwdRoot::Unc(server.to_ascii_lowercase(), share.to_ascii_lowercase()),
            rest,
            true,
        )
    } else if b.len() >= 3
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (b[2] == b'\\' || b[2] == b'/')
    {
        (CwdRoot::Drive(b[0].to_ascii_uppercase()), &s[3..], true)
    } else {
        (CwdRoot::Unix, s.strip_prefix('/')?, false)
    };
    let sep: &[char] = if win_sep { &['/', '\\'] } else { &['/'] };
    let mut parts: Vec<String> = Vec::new();
    for part in rest.split(sep) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p.to_owned()),
        }
    }
    if root == CwdRoot::Unix
        && parts.first().map(String::as_str) == Some("private")
        && matches!(
            parts.get(1).map(String::as_str),
            Some("tmp" | "var" | "etc")
        )
    {
        parts.remove(0);
    }
    Some((root, parts))
}

pub fn cwd_within(folder: &str, cwd: &str) -> bool {
    let Some((f_root, f_parts)) = parse_cwd(folder) else {
        return false;
    };
    let Some((c_root, c_parts)) = parse_cwd(cwd) else {
        return false;
    };
    f_root == c_root && f_parts.len() <= c_parts.len() && c_parts[..f_parts.len()] == f_parts[..]
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
