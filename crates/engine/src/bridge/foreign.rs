//! Codex and Antigravity CLI sessions on the phone (Swift `ForeignSessions`, the pure part): Chat's
//! rows and conversations mapped onto the bridge's sessions and chat messages. Their ids are
//! Chat's row ids (`codex:<id>`, `agy:<id>`), `control` is `job`, and `tool` says which tool.
//!
//! Message ids are hashed from the session and item ids, so an item keeps its id between reads (the
//! hub pushes a message once, by id), and they equal the Swift app's for the same item.

use super::model::*;
use crate::transcript::model::{Conversation, DiffKind, ItemKind, ToolCall, ToolKind};
use notchling_protocol::generated as wire;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Rows older than this aren't listed.
pub const MAX_AGE_MS: f64 = 7.0 * 86_400_000.0;
/// At most this many rows of each tool.
pub const PER_TOOL: usize = 30;
/// An Antigravity permission's request id: `agy-perm:<row id>:<rpc id>`.
pub const PROMPT_PREFIX: &str = "agy-perm:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    Codex,
    Agy,
}

impl Tool {
    pub fn id_prefix(self) -> &'static str {
        match self {
            Tool::Codex => "codex:",
            Tool::Agy => "agy:",
        }
    }

    /// `tool` on the wire.
    pub fn name(self) -> &'static str {
        match self {
            Tool::Codex => "codex",
            Tool::Agy => "agy",
        }
    }
}

/// A Codex or Antigravity row as Chat lists it (Swift `Agent` with a `.codex`/`.antigravity` target).
#[derive(Debug, Clone, PartialEq)]
pub struct ForeignRow {
    /// `codex:<id>` or `agy:<id>`.
    pub id: String,
    pub name: String,
    pub project: String,
    pub cwd: String,
    /// When it was last used, in milliseconds since the epoch.
    pub since_ms: f64,
    /// The row's own group says working (a Codex rollout whose turn is still open).
    pub working: bool,
    pub transcript_path: Option<String>,
}

/// One answer an Antigravity permission offers.
#[derive(Debug, Clone, PartialEq)]
pub struct PermissionOption {
    pub id: String,
    pub name: String,
    /// `allow_once`, `allow_always`, `reject_once`, `reject_always`.
    pub kind: String,
}

impl PermissionOption {
    pub fn allows(&self) -> bool {
        self.kind.starts_with("allow")
    }
}

/// Antigravity's `session/request_permission`, waiting.
#[derive(Debug, Clone, PartialEq)]
pub struct AcpPermission {
    /// The JSON-RPC id, as text.
    pub rpc_id: String,
    pub call: ToolCall,
    pub options: Vec<PermissionOption>,
}

/// `codex:abc` gives `(Codex, "abc")`; None for a Claude Code session id.
pub fn parse(row_id: &str) -> Option<(Tool, String)> {
    [Tool::Codex, Tool::Agy].into_iter().find_map(|tool| {
        let id = row_id.strip_prefix(tool.id_prefix())?;
        (!id.is_empty()).then(|| (tool, id.to_string()))
    })
}

/// Rows worth showing: not closed in Chat (`closed`: row id to when, in seconds; a row comes back
/// once used after that), used within `max_age_ms`, newest first, at most `per_tool` of each.
pub fn visible(
    rows: &[ForeignRow],
    closed: &HashMap<String, f64>,
    now_ms: f64,
    max_age_ms: f64,
    per_tool: usize,
) -> Vec<ForeignRow> {
    let mut sorted: Vec<&ForeignRow> = rows.iter().collect();
    sorted.sort_by(|a, b| b.since_ms.total_cmp(&a.since_ms));
    let mut counts: HashMap<Tool, usize> = HashMap::new();
    let mut out = Vec::new();
    for row in sorted {
        let Some((tool, _)) = parse(&row.id) else {
            continue;
        };
        if now_ms - row.since_ms >= max_age_ms {
            continue;
        }
        if closed
            .get(&row.id)
            .is_some_and(|at| row.since_ms / 1000.0 <= *at)
        {
            continue;
        }
        let count = counts.entry(tool).or_default();
        if *count >= per_tool {
            continue;
        }
        *count += 1;
        out.push(row.clone());
    }
    out
}

/// A row as a bridge session. `working` is the runners' live answer, which beats the row's own;
/// a waiting permission makes it need input.
pub fn session(row: &ForeignRow, working: bool, pending: Option<PendingPrompt>) -> Option<Session> {
    let (tool, _) = parse(&row.id)?;
    let state = if pending.is_some() {
        SessionState::NeedsInput
    } else if working {
        SessionState::Running
    } else {
        SessionState::Done
    };
    // The phone treats `job` as a session it can type into; sends are routed by `tool`.
    let mut session = Session::new(
        &row.id,
        &row.name,
        &row.cwd,
        Control::Job(row.id.clone()),
        state,
    );
    session.project = row.project.clone();
    session.status_updated_ms = row.since_ms;
    session.started_ms = row.since_ms;
    session.transcript_path = row.transcript_path.clone();
    session.pending = pending;
    session.tool = tool.name().into();
    Some(session)
}

/// The newest assistant text and the context size, for the list and the chat header.
pub fn enrich(session: &mut Session, conversation: &Conversation) {
    let last_reply = conversation
        .items
        .iter()
        .rev()
        .find_map(|item| match &item.kind {
            ItemKind::Assistant { text } => Some((item.id.as_str(), text.as_str())),
            _ => None,
        });
    if let Some((id, text)) = last_reply {
        session.last_message = Some(LastMessage {
            id: stable_uuid(&format!("{}|{id}", session.id)),
            text: text.trim().chars().take(240).collect(),
        });
    }
    if session.state == SessionState::Running {
        session.activity = conversation.activity.clone();
    }
    let facts = &conversation.facts;
    if facts.context_tokens > 0 || facts.model.is_some() {
        session.facts = Some(Facts {
            context_tokens: (facts.context_tokens > 0).then_some(facts.context_tokens as i64),
            context_window: None,
            model: facts.model.clone(),
            limits: Vec::new(),
        });
    }
}

/// A conversation as chat messages. `hold_tail` drops a last item that may still grow (a streaming
/// reply, a tool row gaining calls), so it goes out whole once the turn ends.
pub fn messages(
    conversation: &Conversation,
    session_id: &str,
    fallback_ms: f64,
    hold_tail: bool,
) -> Vec<ChatMessage> {
    let mut items = conversation.items.as_slice();
    if hold_tail {
        if let Some(last) = items.last() {
            if matches!(
                last.kind,
                ItemKind::Assistant { .. } | ItemKind::Tools(_) | ItemKind::Thinking { .. }
            ) {
                items = &items[..items.len() - 1];
            }
        }
    }
    let mut out = Vec::new();
    let mut last_ms = fallback_ms;
    for item in items {
        let time = item.date.map_or(last_ms, |seconds| seconds * 1000.0);
        last_ms = time;
        let id = stable_uuid(&format!("{session_id}|{}", item.id));
        let message = |role, text: &str| ChatMessage {
            id: id.clone(),
            role,
            text: text.to_string(),
            time,
            tool_summary: None,
            queued: false,
            images: Vec::new(),
            kind: None,
            steps: None,
            output: None,
        };
        match &item.kind {
            ItemKind::User { text, .. } => out.push(message(wire::ChatRole::User, text)),
            ItemKind::Assistant { text }
            | ItemKind::Notice { text, .. }
            | ItemKind::CommandOutput { text } => {
                out.push(message(wire::ChatRole::Assistant, text))
            }
            ItemKind::Tools(calls) => {
                let summary = tool_summary(calls);
                let mut tools = message(wire::ChatRole::Tools, &summary);
                tools.tool_summary = Some(summary);
                tools.steps = (!calls.is_empty()).then(|| calls.iter().map(step).collect());
                out.push(tools);
            }
            ItemKind::Thinking { .. } | ItemKind::TurnEnd { .. } => {}
        }
    }
    out
}

/// One call as a chat step: output capped at 6,000 characters, at most 120 diff rows, hunk headers
/// dropped, and the first absolute path of a read or edit as its path.
pub fn step(call: &ToolCall) -> wire::ToolStep {
    let diff: Vec<String> = call
        .diff
        .iter()
        .take(120)
        .filter_map(|line| {
            let mark = match line.kind {
                DiffKind::Add => '+',
                DiffKind::Remove => '-',
                DiffKind::Context => ' ',
                DiffKind::Hunk => return None,
            };
            Some(format!("{mark}{}", line.text))
        })
        .collect();
    let output = call.output.as_ref().map(|text| {
        if text.chars().count() > 6000 {
            format!("{}\n…", text.chars().take(6000).collect::<String>())
        } else {
            text.clone()
        }
    });
    let path = matches!(call.kind, ToolKind::Edit | ToolKind::Write | ToolKind::Read)
        .then(|| {
            call.input
                .lines()
                .find(|l| l.starts_with('/'))
                .map(String::from)
        })
        .flatten();
    wire::ToolStep {
        name: call.name.clone(),
        summary: call.title.chars().take(200).collect(),
        output,
        path,
        diff: (!diff.is_empty()).then_some(diff),
    }
}

/// "Ran 2 shell commands, edited 1 file", as the Claude Code chat words it.
pub fn tool_summary(calls: &[ToolCall]) -> String {
    let mut order: Vec<ToolKind> = Vec::new();
    let mut counts: Vec<usize> = Vec::new();
    for call in calls {
        match order.iter().position(|k| *k == call.kind) {
            Some(i) => counts[i] += 1,
            None => {
                order.push(call.kind);
                counts.push(1);
            }
        }
    }
    let n = |total: usize, one: &str, many: &str| {
        format!("{total} {}", if total == 1 { one } else { many })
    };
    let parts: Vec<String> = order
        .iter()
        .zip(&counts)
        .map(|(kind, &total)| match kind {
            ToolKind::Shell => format!("ran {}", n(total, "shell command", "shell commands")),
            ToolKind::Read => format!("read {}", n(total, "file", "files")),
            ToolKind::Edit => format!("edited {}", n(total, "file", "files")),
            ToolKind::Write => format!("wrote {}", n(total, "file", "files")),
            ToolKind::Search => format!("searched for {}", n(total, "pattern", "patterns")),
            ToolKind::Web => "searched the web".into(),
            ToolKind::Agent => format!("ran {}", n(total, "agent", "agents")),
            ToolKind::Task => "updated tasks".into(),
            ToolKind::Ask => "asked you".into(),
            ToolKind::Skill => format!("used {}", n(total, "skill", "skills")),
            ToolKind::Other => format!("used {}", n(total, "tool", "tools")),
        })
        .collect();
    let text = parts.join(", ");
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => text,
    }
}

/// Antigravity's permission as the bridge's: the phone gets Allow and Deny, which answer with the
/// permission's own one-time options (Always Allow and the like stay on the computer).
pub fn prompt(permission: &AcpPermission, row_id: &str, created_ms: f64) -> PendingPrompt {
    let call = &permission.call;
    let source = if call.input.is_empty() {
        &call.title
    } else {
        &call.input
    };
    let line = source
        .lines()
        .find(|l| !l.is_empty())
        .unwrap_or(&call.title);
    PendingPrompt {
        id: format!("{PROMPT_PREFIX}{row_id}:{}", permission.rpc_id),
        session_id: row_id.to_string(),
        kind: PromptKind::Permission,
        tool_name: call.name.clone(),
        summary: format!("{}: {line}", call.title)
            .chars()
            .take(300)
            .collect(),
        created_ms,
    }
}

/// `(row id, rpc id)` from an id made by [`prompt`]. The row id is `agy:<uuid>`, so the rpc id
/// starts after the second colon and may hold colons of its own.
pub fn parse_prompt(id: &str) -> Option<(String, String)> {
    let rest = id.strip_prefix(PROMPT_PREFIX)?;
    let mut pieces = rest.splitn(3, ':');
    let (tool, row, rpc) = (pieces.next()?, pieces.next()?, pieces.next()?);
    (!rpc.is_empty()).then(|| (format!("{tool}:{row}"), rpc.to_string()))
}

/// The option that answers `response`; None means cancel the turn (no option of that kind).
pub fn option(response: &PromptResponse, permission: &AcpPermission) -> Option<String> {
    let find = |kind: &str, allows: bool| {
        permission
            .options
            .iter()
            .find(|o| o.kind == kind)
            .or_else(|| permission.options.iter().find(|o| o.allows() == allows))
            .map(|o| o.id.clone())
    };
    match response {
        PromptResponse::Allow => find("allow_once", true),
        PromptResponse::Deny => find("reject_once", false),
        PromptResponse::Answers(_) => None,
    }
}

/// The first 16 bytes of SHA-256 of `seed` as an uppercase UUID string (Swift `stableUUID`).
pub fn stable_uuid(seed: &str) -> String {
    let digest = Sha256::digest(seed.as_bytes());
    let hex: String = digest[..16].iter().map(|b| format!("{b:02X}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}
