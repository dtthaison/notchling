//! The backend the daemon runs the bridge on (Swift `LiveBridgeBackend` with `PhoneModel` and
//! `ForeignSessionSource`): sessions with what Notchling knows about them (activity, facts, the
//! newest reply), chats read from the transcripts, Codex and Antigravity rows beside Claude Code's.
//!
//! What it reads comes from [`Sources`] (the daemon's session list, hostd terminals, held prompts,
//! limits, runners). What it does goes through [`Acting`], which the daemon implements on its
//! acting ops, so the acting switch applies: while it's off, nothing is typed, answered, started or
//! taken over. [`ActingOff`] is that state on its own.
//!
//! The phone extras (E15) are [`Extras`]: viz specs, chat pictures, web snapshots and the
//! device-window stream. Capture (a page, a window) is behind traits whose default answers
//! "not supported on this computer".

use super::backend::BridgeBackend;
use super::connection::{lock, Connection};
use super::extras::{wire_image, Extras};
use super::foreign::{self, AcpPermission, ForeignRow, Tool};
use super::model::*;
use super::wire::BridgeFailure;
use crate::transcript::{chat, codex, conversation, model::Conversation, snapshot};
use notchling_protocol::generated as wire;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Sessions quiet longer than this aren't enriched (their transcripts aren't read on every push).
const ENRICH_WITHIN_MS: f64 = 6.0 * 3_600_000.0;

/// What the runners know right now about the listed Codex and Antigravity rows.
#[derive(Debug, Clone, Default)]
pub struct ForeignLive {
    /// Codex session ids (not row ids) with a turn running.
    pub codex_running: HashSet<String>,
    /// Antigravity session ids with a turn running.
    pub agy_working: HashSet<String>,
    /// Waiting permissions, by row id.
    pub permissions: HashMap<String, AcpPermission>,
    /// Open Antigravity conversations, by row id.
    pub conversations: HashMap<String, Conversation>,
}

/// What the bridge reads from the daemon. Every call is cheap or cached by the implementation; the
/// session list is asked for on every 300 ms tick.
pub trait Sources: Send + Sync {
    /// Claude Code sessions with their held prompts, plus owned terminals that aren't a session yet
    /// (`PhoneModel.bridgeSessions`, ids `pty:<uuid>`). Not enriched.
    fn claude_sessions(&self) -> Vec<Session>;
    /// Whether a view-only session can be taken over (`Takeover.state`).
    fn takeover_state(&self, _session: &Session) -> Option<TakeoverState> {
        None
    }
    /// The active account's limits, for the chat header.
    fn limits(&self) -> Vec<Limit> {
        Vec::new()
    }
    /// Chat's Codex and Antigravity rows.
    fn foreign_rows(&self) -> Vec<ForeignRow> {
        Vec::new()
    }
    /// Rows closed in Chat: row id to when, in seconds (`chatClosedSessions`).
    fn closed_rows(&self) -> HashMap<String, f64> {
        HashMap::new()
    }
    fn foreign_live(&self, _rows: &[ForeignRow]) -> ForeignLive {
        ForeignLive::default()
    }
    /// The slash commands a session can run (Claude Code's for its folder, Antigravity's own).
    fn commands(&self, _session: &Session) -> Vec<Command> {
        Vec::new()
    }
    /// The Claude models a new session may start on now (the catalog's current ones).
    fn models(&self) -> Vec<ModelChoice> {
        Vec::new()
    }
    /// Whether a session may start on this model id, by the catalog's own check.
    fn model_allowed(&self, _id: &str) -> bool {
        false
    }
    /// `~/.claude/projects`, for the recent folders.
    fn projects_dir(&self) -> Option<PathBuf> {
        None
    }
    /// Asks the person at the computer to approve a phone. No approver means no.
    fn request_approval(&self, _device_name: &str, _platform: &str, _address: &str) -> bool {
        false
    }
    fn now_ms(&self) -> f64;
}

/// What the bridge does to sessions. The daemon's implementation runs its acting ops, which refuse
/// while acting is off. Messages to Codex and Antigravity rows come here too (`session.tool`).
pub trait Acting: Send + Sync {
    /// The acting switch now; the calls below still refuse on their own while it's off.
    fn is_on(&self) -> bool {
        true
    }
    fn send_message(&self, text: &str, session: &Session) -> Result<(), BridgeFailure>;
    fn send_now(&self, session: &Session) -> Result<(), BridgeFailure>;
    fn respond(
        &self,
        prompt: &PendingPrompt,
        response: &PromptResponse,
    ) -> Result<(), BridgeFailure>;
    fn new_session(&self, cwd: &Path, model: Option<&str>) -> Result<(), BridgeFailure>;
    /// A new session on a model id the catalog allows (`session.new`'s `modelId`).
    fn new_session_with_model(&self, _cwd: &Path, _model_id: &str) -> Result<(), BridgeFailure> {
        Err(BridgeFailure::invalid(
            "Backend này không chọn được model theo id.",
        ))
    }
    fn take_over(
        &self,
        session: &Session,
        stop_original: bool,
    ) -> Result<TakeoverOutcome, BridgeFailure>;
}

/// What every acting call answers while acting is off.
pub const ACTING_OFF: &str =
    "Máy này chưa bật quyền điều khiển session: ứng dụng Notchling cũ vẫn giữ chúng.";

/// Acting switched off: every call is refused and nothing happens.
pub struct ActingOff;

fn off<T>() -> Result<T, BridgeFailure> {
    Err(BridgeFailure::invalid(ACTING_OFF))
}

impl Acting for ActingOff {
    fn is_on(&self) -> bool {
        false
    }
    fn send_message(&self, _: &str, _: &Session) -> Result<(), BridgeFailure> {
        off()
    }
    fn send_now(&self, _: &Session) -> Result<(), BridgeFailure> {
        off()
    }
    fn respond(&self, _: &PendingPrompt, _: &PromptResponse) -> Result<(), BridgeFailure> {
        off()
    }
    fn new_session(&self, _: &Path, _: Option<&str>) -> Result<(), BridgeFailure> {
        off()
    }
    fn new_session_with_model(&self, _: &Path, _: &str) -> Result<(), BridgeFailure> {
        off()
    }
    fn take_over(&self, _: &Session, _: bool) -> Result<TakeoverOutcome, BridgeFailure> {
        off()
    }
}

pub struct LiveBackend<S: Sources, A: Acting> {
    pub sources: S,
    pub acting: A,
    /// When each Antigravity permission was first seen, so its `created` stays put between pushes.
    prompt_seen: Mutex<HashMap<String, f64>>,
    pub(super) extras: Extras,
}

impl<S: Sources, A: Acting> LiveBackend<S, A> {
    /// With [`Extras::real`]: the real viz engine and picture encoder, no page or window capture.
    pub fn new(sources: S, acting: A) -> LiveBackend<S, A> {
        LiveBackend {
            sources,
            acting,
            prompt_seen: Mutex::new(HashMap::new()),
            extras: Extras::real(notchling_platform::paths::home().unwrap_or_default()),
        }
    }

    /// Replaces the extras (the daemon's capture, a test's fakes).
    pub fn with_extras(mut self, extras: Extras) -> LiveBackend<S, A> {
        self.extras = extras;
        self
    }

    fn first_seen(&self, row_id: &str, rpc_id: &str, now_ms: f64) -> f64 {
        let mut seen = lock(&self.prompt_seen);
        if seen.len() > 50 {
            seen.retain(|_, at| now_ms - *at < 3_600_000.0);
        }
        *seen.entry(format!("{row_id}:{rpc_id}")).or_insert(now_ms)
    }

    fn foreign_sessions(&self, now_ms: f64) -> Vec<Session> {
        let rows = foreign::visible(
            &self.sources.foreign_rows(),
            &self.sources.closed_rows(),
            now_ms,
            foreign::MAX_AGE_MS,
            foreign::PER_TOOL,
        );
        if rows.is_empty() {
            return Vec::new();
        }
        let live = self.sources.foreign_live(&rows);
        rows.iter()
            .filter_map(|row| {
                let (tool, id) = foreign::parse(&row.id)?;
                let pending = live.permissions.get(&row.id).map(|p| {
                    foreign::prompt(p, &row.id, self.first_seen(&row.id, &p.rpc_id, now_ms))
                });
                let working = match tool {
                    Tool::Codex => live.codex_running.contains(&id) || row.working,
                    Tool::Agy => live.agy_working.contains(&id),
                };
                let mut session = foreign::session(row, working, pending)?;
                match (tool, &row.transcript_path) {
                    (Tool::Codex, Some(path)) if now_ms - row.since_ms < ENRICH_WITHIN_MS => {
                        foreign::enrich(&mut session, &codex::read(path));
                    }
                    _ => {
                        if let Some(conversation) = live.conversations.get(&row.id) {
                            foreign::enrich(&mut session, conversation);
                        }
                    }
                }
                Some(session)
            })
            .collect()
    }

    fn foreign_messages(&self, row_id: &str) -> Result<Vec<ChatMessage>, BridgeFailure> {
        let rows = self.sources.foreign_rows();
        let row = rows
            .iter()
            .find(|r| r.id == row_id)
            .ok_or_else(|| BridgeFailure::not_found("Không có session này."))?;
        let (tool, id) = foreign::parse(row_id)
            .ok_or_else(|| BridgeFailure::not_found("Không có session này."))?;
        let live = self.sources.foreign_live(std::slice::from_ref(row));
        let (conversation, running) = match tool {
            Tool::Codex => {
                let path = row
                    .transcript_path
                    .as_deref()
                    .ok_or_else(|| BridgeFailure::not_found("Chưa tìm thấy transcript."))?;
                (codex::read(path), live.codex_running.contains(&id))
            }
            Tool::Agy => (
                live.conversations.get(row_id).cloned().unwrap_or_default(),
                live.agy_working.contains(&id),
            ),
        };
        Ok(foreign::messages(
            &conversation,
            row_id,
            row.since_ms,
            running,
        ))
    }

    pub(super) fn find(&self, id: &str) -> Result<Session, BridgeFailure> {
        self.sessions()
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| BridgeFailure::not_found("Không có session này."))
    }
}

impl<S: Sources, A: Acting> BridgeBackend for LiveBackend<S, A> {
    fn sessions(&self) -> Vec<Session> {
        let now = self.sources.now_ms();
        let limits = self.sources.limits();
        let mut list: Vec<Session> = self
            .sources
            .claude_sessions()
            .into_iter()
            .map(|mut session| {
                if session.control == Control::ViewOnly {
                    session.takeover = self.sources.takeover_state(&session);
                }
                enrich(&mut session, &limits, now);
                session
            })
            .collect();
        list.extend(self.foreign_sessions(now));
        list
    }

    fn chat_messages(
        &self,
        session_id: &str,
        subagent_id: Option<&str>,
    ) -> Result<Vec<ChatMessage>, BridgeFailure> {
        if foreign::parse(session_id).is_some() {
            return self.foreign_messages(session_id);
        }
        let session = self.find(session_id)?;
        let path = match subagent_id {
            Some(sub) => session
                .subagents
                .iter()
                .find(|s| s.info.id == sub)
                .and_then(|s| s.transcript_path.clone()),
            None => session.transcript_path.clone(),
        };
        let path = path.ok_or_else(|| BridgeFailure::not_found("Chưa tìm thấy transcript."))?;
        let read = chat::read_in(
            &path,
            subagent_id.is_none(),
            chat::DEFAULT_LIMIT,
            &self.extras.home,
        );
        Ok(read.messages.iter().map(chat_message).collect())
    }

    fn commands(&self, session_id: &str) -> Vec<Command> {
        match self.find(session_id) {
            Ok(session) if foreign::parse(session_id).is_none_or(|(tool, _)| tool == Tool::Agy) => {
                self.sources.commands(&session)
            }
            _ => Vec::new(),
        }
    }

    fn chat_image(
        &self,
        session_id: &str,
        image_id: &str,
        max_width: i64,
    ) -> Result<Vec<u8>, BridgeFailure> {
        self.picture(session_id, image_id, max_width)
    }

    fn viz(&self, session_id: &str, include_entity: bool) -> Result<BridgeViz, BridgeFailure> {
        self.viz_bundle(session_id, include_entity)
    }

    fn web_snapshot(&self, url: &str, width: i64) -> Result<Vec<u8>, BridgeFailure> {
        self.extras.snapshots.snapshot(url, width)
    }

    fn send_message(&self, text: &str, session: &Session) -> Result<(), BridgeFailure> {
        self.acting.send_message(text, session)
    }

    fn acting_on(&self) -> bool {
        self.acting.is_on()
    }

    fn send_now(&self, session: &Session) -> Result<(), BridgeFailure> {
        self.acting.send_now(session)
    }

    fn pending_prompt(&self, request_id: &str) -> Option<PendingPrompt> {
        if let Some((row_id, rpc_id)) = foreign::parse_prompt(request_id) {
            let row = self
                .sources
                .foreign_rows()
                .into_iter()
                .find(|r| r.id == row_id)?;
            let live = self.sources.foreign_live(std::slice::from_ref(&row));
            let permission = live
                .permissions
                .get(&row_id)
                .filter(|p| p.rpc_id == rpc_id)?;
            let created = self.first_seen(&row_id, &rpc_id, self.sources.now_ms());
            return Some(foreign::prompt(permission, &row_id, created));
        }
        self.sources
            .claude_sessions()
            .into_iter()
            .find_map(|s| s.pending.filter(|p| p.id == request_id))
    }

    fn respond(
        &self,
        prompt: &PendingPrompt,
        response: &PromptResponse,
    ) -> Result<(), BridgeFailure> {
        self.acting.respond(prompt, response)
    }

    fn new_session(&self, cwd: &Path, model: Option<&str>) -> Result<(), BridgeFailure> {
        self.acting.new_session(cwd, model)
    }

    fn models(&self) -> Vec<ModelChoice> {
        self.sources.models()
    }

    fn model_allowed(&self, id: &str) -> bool {
        self.sources.model_allowed(id)
    }

    fn new_session_with_model(&self, cwd: &Path, model_id: &str) -> Result<(), BridgeFailure> {
        self.acting.new_session_with_model(cwd, model_id)
    }

    fn take_over(
        &self,
        session: &Session,
        stop_original: bool,
    ) -> Result<TakeoverOutcome, BridgeFailure> {
        self.acting.take_over(session, stop_original)
    }

    fn recent_projects(&self) -> Vec<String> {
        let sessions = self.sessions();
        let projects = self.sources.projects_dir();
        recent_projects(&sessions, projects.as_deref(), 30, &|p| {
            Path::new(p).is_dir()
        })
    }

    fn request_approval(&self, device_name: &str, platform: &str, address: &str) -> bool {
        self.sources
            .request_approval(device_name, platform, address)
    }

    fn start_phone(
        &self,
        connection: &Arc<Connection>,
        options: &PhoneOptions,
    ) -> Result<(), BridgeFailure> {
        self.extras.streamer.start(connection, options)
    }

    fn stop_phone(&self, connection: &Arc<Connection>) {
        self.extras.streamer.stop(connection);
    }
}

/// What Notchling knows about a Claude Code session, for the list and the chat header
/// (`LiveBridgeBackend.enrich`): what it's doing while it runs, its newest reply, the context in
/// use and the account's limits. Only sessions active in the last six hours; the readers keep a
/// cursor per transcript, so a repeat call reads only what was appended.
pub fn enrich(session: &mut Session, limits: &[Limit], now_ms: f64) {
    if session.tool != "claude" || session.state == SessionState::Exited {
        return;
    }
    let Some(path) = session.transcript_path.clone() else {
        return;
    };
    if now_ms - session.last_activity_ms() >= ENRICH_WITHIN_MS {
        return;
    }
    if session.state == SessionState::Running {
        session.activity = snapshot::read(&path).activity;
    }
    let read = conversation::read(&path);
    let reply = read.items.iter().rev().find_map(|item| match &item.kind {
        crate::transcript::model::ItemKind::Assistant { text } => {
            Some((item.id.clone(), text.clone()))
        }
        _ => None,
    });
    if let Some((id, text)) = reply {
        session.last_message = Some(LastMessage {
            id,
            text: text.trim().chars().take(240).collect(),
        });
    }
    let facts = &read.facts;
    if facts.context_tokens == 0 && limits.is_empty() {
        return;
    }
    session.facts = Some(Facts {
        context_tokens: (facts.context_tokens > 0).then_some(facts.context_tokens as i64),
        context_window: context_window(facts.model.as_deref()),
        model: facts.model.clone(),
        limits: limits.to_vec(),
    });
}

/// The model's context window where it's known (Opus 5.5: 400k), as Chat's header has it.
pub fn context_window(model: Option<&str>) -> Option<i64> {
    model
        .filter(|m| m.starts_with("claude-opus-5-5"))
        .map(|_| 400_000)
}

/// A transcript chat message on the wire; its id is the Swift app's (`stableUUID` of its seed).
pub fn chat_message(message: &chat::ChatMessage) -> ChatMessage {
    let steps: Vec<wire::ToolStep> = message
        .steps
        .iter()
        .map(|s| wire::ToolStep {
            name: s.name.clone(),
            summary: s.summary.clone(),
            output: s.output.clone(),
            path: s.path.clone(),
            diff: (!s.diff.is_empty()).then(|| s.diff.iter().map(|d| d.wire()).collect()),
        })
        .collect();
    ChatMessage {
        id: foreign::stable_uuid(&message.id),
        role: match message.role {
            chat::ChatRole::User => wire::ChatRole::User,
            chat::ChatRole::Assistant => wire::ChatRole::Assistant,
            chat::ChatRole::Tools => wire::ChatRole::Tools,
        },
        text: message.text.clone(),
        time: message.timestamp * 1000.0,
        tool_summary: message.tool_summary.clone(),
        queued: message.queued,
        images: message.images.iter().map(wire_image).collect(),
        kind: match message.kind {
            chat::ChatKind::Text => None,
            chat::ChatKind::Shell => Some(wire::ChatKind::Shell),
            chat::ChatKind::Compaction => Some(wire::ChatKind::Compaction),
        },
        steps: (!steps.is_empty()).then_some(steps),
        output: message.output.clone(),
    }
}

/// Recent distinct project folders: the known sessions' folders first, newest first, then folders
/// recovered from the names in `~/.claude/projects`, newest first (Swift `RecentProjects.list`).
pub fn recent_projects(
    sessions: &[Session],
    projects_dir: Option<&Path>,
    limit: usize,
    exists: &dyn Fn(&str) -> bool,
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result: Vec<String> = Vec::new();
    let mut add = |path: String, result: &mut Vec<String>| {
        if !path.is_empty() && path != "/" && !seen.contains(&path) && exists(&path) {
            seen.insert(path.clone());
            result.push(path);
        }
    };
    let mut ordered: Vec<&Session> = sessions.iter().collect();
    ordered.sort_by(|a, b| b.last_activity_ms().total_cmp(&a.last_activity_ms()));
    for session in ordered {
        add(session.cwd.clone(), &mut result);
    }
    let mut entries: Vec<(String, std::time::SystemTime)> = projects_dir
        .and_then(|dir| std::fs::read_dir(dir).ok())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            Some((
                e.file_name().to_str()?.to_string(),
                e.metadata().ok()?.modified().ok()?,
            ))
        })
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.1));
    for (name, _) in entries {
        if result.len() >= limit {
            break;
        }
        if let Some(path) = decode_project_dir(&name, exists) {
            add(path, &mut result);
        }
    }
    result.truncate(limit);
    result
}

/// `-Users-me-my-app` is `/Users/me/my-app` or `/Users/me/my/app`: dash-separated parts are joined
/// into one folder name wherever that folder exists.
pub fn decode_project_dir(name: &str, exists: &dyn Fn(&str) -> bool) -> Option<String> {
    let parts: Vec<&str> = name.strip_prefix('-')?.split('-').collect();
    fn search(
        parts: &[&str],
        index: usize,
        base: &str,
        exists: &dyn Fn(&str) -> bool,
    ) -> Option<String> {
        if index == parts.len() {
            return Some(base.to_string());
        }
        let mut folder = String::new();
        for (end, part) in parts.iter().enumerate().skip(index) {
            if end > index {
                folder.push('-');
            }
            folder.push_str(part);
            let candidate = format!("{base}/{folder}");
            if exists(&candidate) {
                if let Some(found) = search(parts, end + 1, &candidate, exists) {
                    return Some(found);
                }
            }
        }
        None
    }
    search(&parts, 0, "", exists)
}
