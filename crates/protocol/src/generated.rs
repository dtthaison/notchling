//! The `agentconsole.v1` wire types, generated from `protocol/agentconsole.v1.schema.json` by
//! `tools/protocol/gen.py`. @generated: do not edit by hand; change the schema and run the generator.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Why the Mac refused a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorCode {
    #[serde(rename = "unauthorized")]
    Unauthorized,
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "view_only")]
    ViewOnly,
    #[serde(rename = "invalid")]
    Invalid,
    #[serde(rename = "busy")]
    Busy,
    #[serde(rename = "not_reachable")]
    NotReachable,
    #[serde(rename = "internal")]
    Internal,
    #[serde(other)]
    Unknown,
}

/// What the session is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionState {
    #[serde(rename = "needsInput")]
    NeedsInput,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "exited")]
    Exited,
    #[serde(other)]
    Unknown,
}

/// How the Mac can reach a session: its own pty (console), a background job (job), or not at all (viewOnly).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionControl {
    #[serde(rename = "console")]
    Console,
    #[serde(rename = "job")]
    Job,
    #[serde(rename = "viewOnly")]
    ViewOnly,
    #[serde(other)]
    Unknown,
}

/// Who wrote a chat message. Tool calls of one step arrive as one tools message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChatRole {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant,
    #[serde(rename = "tools")]
    Tools,
    #[serde(other)]
    Unknown,
}

/// A `!` shell command (text is the command, output what it printed) or the notice that the conversation was compacted. Absent for a normal message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChatKind {
    #[serde(rename = "shell")]
    Shell,
    #[serde(rename = "compaction")]
    Compaction,
    #[serde(other)]
    Unknown,
}

/// A permission (allow or deny) or a question with options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PromptKind {
    #[serde(rename = "permission")]
    Permission,
    #[serde(rename = "question")]
    Question,
    #[serde(other)]
    Unknown,
}

/// Whether the subagent is still working.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SubagentState {
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "done")]
    Done,
    #[serde(other)]
    Unknown,
}

/// Where a chat picture came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ImageOrigin {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "toolResult")]
    ToolResult,
    #[serde(rename = "file")]
    File,
    #[serde(other)]
    Unknown,
}

/// Which tab of the viz bundle a spec fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VizKind {
    #[serde(rename = "flow")]
    Flow,
    #[serde(rename = "diagram")]
    Diagram,
    #[serde(rename = "entity")]
    Entity,
    #[serde(rename = "chart")]
    Chart,
    #[serde(other)]
    Unknown,
}

/// A subagent a session started.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subagent {
    pub id: String,
    /// The agent type, for example Explore.
    pub r#type: String,
    pub description: String,
    pub model: String,
    /// 1 for a subagent of the session, 2 for one of its subagents.
    pub depth: i64,
    #[serde(default)]
    pub finished: bool,
    pub state: SubagentState,
    /// Milliseconds since 1970-01-01 UTC.
    pub last_activity: f64,
}

/// One question of an AskUserQuestion prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptQuestion {
    pub header: String,
    pub question: String,
    pub options: Vec<String>,
    pub multi_select: bool,
}

/// A question or permission a hook on the Mac is waiting on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub request_id: String,
    pub session_id: String,
    pub kind: PromptKind,
    pub tool_name: String,
    pub summary: String,
    /// Empty for a permission.
    pub questions: Vec<PromptQuestion>,
    /// Milliseconds since 1970-01-01 UTC.
    pub created: f64,
}

/// Whether the Mac can take a view-only session over. Only on view-only sessions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Takeover {
    /// Can be resumed right now.
    pub available: bool,
    /// Why not, when it can't.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The original is still running but the Mac can stop it (session.takeover with stopOriginal) and resume.
    #[serde(default)]
    pub stoppable: bool,
    /// The original is mid-turn: stopping it interrupts the turn.
    #[serde(default)]
    pub busy: bool,
}

/// A cheap marker of Claude's newest reply. A new id means a new reply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LastMessage {
    pub id: String,
    /// The first lines of the reply.
    pub text: String,
}

/// One limit of the account a session runs on, as a percentage used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit {
    /// For example 5h or Week.
    pub label: String,
    pub percent: f64,
    /// Milliseconds since 1970-01-01 UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<f64>,
}

/// What the chat header shows: the context in use, the model's window and the account's limits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<i64>,
    /// Absent when the window is unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub limits: Vec<Limit>,
}

/// One session the Mac knows about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub name: String,
    /// Last path component of cwd.
    pub project: String,
    pub cwd: String,
    /// cli, desktop, job, sdk, or the raw entrypoint the session reported.
    pub entrypoint: String,
    pub state: SessionState,
    pub control: SessionControl,
    /// Milliseconds since 1970-01-01 UTC.
    pub status_updated_at: f64,
    /// Milliseconds since 1970-01-01 UTC.
    pub started_at: f64,
    /// Milliseconds since 1970-01-01 UTC.
    pub last_activity: f64,
    #[serde(default)]
    pub subagents: Vec<Subagent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<Prompt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub takeover: Option<Takeover>,
    /// What a working session is doing, for example Editing main.swift.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facts: Option<Facts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message: Option<LastMessage>,
    /// claude, codex or agy. Additive: an older Mac sends none, an older phone ignores it.
    #[serde(default = "default_session_tool")]
    pub tool: String,
}

fn default_session_tool() -> String {
    "claude".to_string()
}

/// A picture a chat message refers to. Only this description travels in the chat; the bytes come from chat.image.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub id: String,
    pub media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<i64>,
    pub origin: ImageOrigin,
    /// Size of the original.
    #[serde(default)]
    pub bytes: i64,
}

/// One tool call inside a tools message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolStep {
    pub name: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// One string per row: "+added", "-removed", " context". Absent when there is no diff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<Vec<String>>,
}

/// One message of a conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub role: ChatRole,
    pub text: String,
    /// Milliseconds since 1970-01-01 UTC.
    pub time: f64,
    /// On tools messages: for example Read 2 files, ran 1 shell command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_summary: Option<String>,
    /// Sent while the agent was working and not yet taken by it.
    #[serde(default)]
    pub queued: bool,
    #[serde(default)]
    pub images: Vec<Image>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ChatKind>,
    /// On tools messages: the calls with input, output and diff. Absent when there are none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<ToolStep>>,
    /// On shell messages: what the command printed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

/// A slash command a session can run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Command {
    /// Without the leading slash.
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
}

/// An empty-state message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VizMessage {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Why a spec could not be built.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VizFailure {
    pub title: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// One tool call in a flow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowCall {
    pub tool: String,
    pub target: String,
    pub subagent: bool,
    pub edit: bool,
    /// Milliseconds since 1970-01-01 UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<f64>,
}

/// One user turn with its calls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowTurn {
    pub index: i64,
    pub prompt: String,
    pub calls: Vec<FlowCall>,
}

/// A file the session touched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowFile {
    pub path: String,
    pub name: String,
    pub count: i64,
}

/// The flow tab: what the session did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_stat: Option<String>,
    pub files: Vec<FlowFile>,
    pub turns: Vec<FlowTurn>,
    pub total_calls: i64,
}

/// A diagram the conversation produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagramBlock {
    pub id: String,
    pub label: String,
    /// Mermaid source.
    pub source: String,
    pub origin: String,
    /// Milliseconds since 1970-01-01 UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<f64>,
}

/// The diagram tab.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagramSpec {
    /// Newest first.
    pub blocks: Vec<DiagramBlock>,
}

/// A schema file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityFile {
    pub path: String,
    pub tables: i64,
}

/// A table of the entity diagram.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityTable {
    /// Identifier as drawn in the diagram.
    pub id: String,
    pub name: String,
    pub columns: i64,
}

/// A relation between tables, in the order of the diagram's lines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityLink {
    pub from: String,
    pub to: String,
    pub inferred: bool,
}

/// The entity tab: a database schema as a Mermaid ER diagram.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitySpec {
    pub mermaid: String,
    pub tables: Vec<EntityTable>,
    pub links: Vec<EntityLink>,
    pub files: Vec<EntityFile>,
    pub table_count: i64,
    pub relation_count: i64,
    pub inferred_count: i64,
    pub warnings: Vec<String>,
}

/// Token use at one moment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsagePoint {
    /// Milliseconds since 1970-01-01 UTC.
    pub time: f64,
    pub input: i64,
    pub output: i64,
    pub cache_creation: i64,
    pub cache_read: i64,
}

/// Token use over the session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTotals {
    pub input: i64,
    pub output: i64,
    pub cache_creation: i64,
    pub cache_read: i64,
}

/// How often a tool ran.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartCount {
    pub name: String,
    pub count: i64,
}

/// How many requests a model served.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCount {
    pub model: String,
    pub count: i64,
}

/// The chart tab.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartSpec {
    pub tokens: Vec<UsagePoint>,
    pub totals: UsageTotals,
    pub tools: Vec<ChartCount>,
    pub models: Vec<ModelCount>,
}

/// What the shared viz bundle renders for one tab. Only the field named by kind is filled, unless empty or error says why not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VizSpec {
    pub kind: VizKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty: Option<VizMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<VizFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<FlowSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagram: Option<DiagramSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity: Option<EntitySpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart: Option<ChartSpec>,
}

/// One button an inbox item offers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxAction {
    /// One of allow, allow_once, allow_always, deny, continue, cancel, done, open. open is for the Mac only: the phone shows it disabled and the Mac refuses it.
    pub id: String,
    pub label: String,
    /// The main button of the item.
    pub primary: bool,
}

/// One notification the Mac's inbox holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxItem {
    pub id: String,
    /// secret_approval, secret_request, limit_notice, action_failed or notice; a phone treats a kind it doesn't know as notice.
    pub kind: String,
    pub title: String,
    pub detail: String,
    /// Where the item came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<InboxItemSource>,
    pub created_ms: i64,
    /// When an open item stops waiting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_ms: Option<i64>,
    /// When the item was answered, expired or withdrawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_ms: Option<i64>,
    pub actions: Vec<InboxAction>,
    /// open, answered, expired or withdrawn.
    pub state: String,
    /// The action id that answered it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// mac or phone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered_by: Option<String>,
    pub seen: bool,
}

/// Where the item came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxItemSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
}

/// First message on a connection from a paired phone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HelloRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
    /// The device token pair returned.
    pub token: String,
}

/// First message from a phone that is pairing with the one-time code; the Mac shows its approval sheet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
    /// The 8-character one-time code.
    pub code: String,
    /// The phone.
    pub device: PairRequestDevice,
}

/// The phone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairRequestDevice {
    pub name: String,
    /// ios, android; defaults to unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
}

/// Keep-alive; answered with the Mac's clock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PingRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
}

/// All sessions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionsListRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
}

/// One page of a conversation: the newest limit messages, or the ones before or after a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGetRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
    /// The session's id.
    pub session_id: String,
    /// Read a subagent's conversation instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_id: Option<String>,
    /// 1 to 200, default 50.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    /// Id of the oldest message the phone has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// Id of the newest message the phone has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

/// The viz specs of a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VizGetRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
    /// The session's id.
    pub session_id: String,
    /// Also build the entity spec.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity: Option<bool>,
}

/// A JPEG of a localhost page. The Mac answers with JSON, then a binary WEB1 frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSnapshotRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
    /// The session's id.
    pub session_id: String,
    /// 100 to 1200, default 400.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<i64>,
    /// An http(s) URL on localhost or 127.0.0.1; absent: the session's own web URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Types a message into a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessageRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
    /// The session's id.
    pub session_id: String,
    /// 1 to 20000 characters.
    pub text: String,
}

/// Interrupts the running turn so the queued message runs next.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSendNowRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
    /// The session's id.
    pub session_id: String,
}

/// Starts a photo upload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionImageBeginRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
    /// The session's id.
    pub session_id: String,
    /// Size of the photo, at most 6 MiB.
    pub bytes: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
}

/// One piece of a photo, in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionImageChunkRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
    pub upload_id: String,
    /// 0, 1, 2, ...
    pub seq: i64,
    /// Base64 of at most 96 KiB.
    pub data: String,
}

/// Finishes a photo upload; its path goes to the session with the caption.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionImageEndRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
    pub upload_id: String,
}

/// The slash commands of a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandsListRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
    /// The session's id.
    pub session_id: String,
}

/// Answers a permission (allow) or a question (choice or text).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptAnswerRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
    pub request_id: String,
    /// For a permission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow: Option<bool>,
    /// For a question: a 1-based option number or an option label; an array with one entry per question.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<Value>,
    /// A free-text answer to a single question, at most 500 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Starts a Claude Code session in a folder inside the Mac's home.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionNewRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
    /// An absolute path.
    pub cwd: String,
    /// default, opus, sonnet or haiku.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// A model id from the `models` of sessions.list, such as claude-opus-5-5 (a [1m] or date suffix is accepted). Wins over `model` when both are sent; the Mac answers invalid for an id it doesn't list. An older Mac ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

/// A JPEG of a picture in the chat. The Mac answers with JSON, then a binary IMG1 frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatImageRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
    /// The session's id.
    pub session_id: String,
    pub image_id: String,
    /// 64 to 1600, default 800.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<i64>,
}

/// Resumes a view-only session in a terminal the Mac owns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTakeoverRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
    /// The session's id.
    pub session_id: String,
    /// Stop the original claude process first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_original: Option<bool>,
}

/// Folders sessions ran in recently.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectsRecentRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
}

/// Replaces what the Mac pushes to this connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
    /// Push the session list and prompts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions: Option<bool>,
    /// Push new chat messages of this session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_id: Option<String>,
    /// The newest message the phone already has, so the ones after it are pushed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message_id: Option<String>,
    /// Stream the Mac's device window as PHN1 frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone: Option<SubscribeRequestPhone>,
    /// Push the inbox's items, and each one raised, changed or removed after. Absent means no inbox pushes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbox: Option<bool>,
}

/// Stream the Mac's device window as PHN1 frames.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeRequestPhone {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_window_id: Option<i64>,
    /// 1 to 15, default 10.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps: Option<i64>,
    /// 100 to 600, default 400.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<i64>,
}

/// Stops every push.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnsubscribeRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
}

/// The Mac's notification inbox.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxListRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
}

/// Presses one action of an open inbox item. The first answer wins, from the Mac or a phone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxAnswerRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub op: String,
    /// The inbox item's id.
    pub item_id: String,
    /// An action id the item offers; open is refused.
    pub action: String,
}

/// The connection is authenticated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HelloResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: HelloResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HelloResponseData {
    /// The paired phone.
    pub device: HelloResponseDataDevice,
    /// The bridge wire version, 1.
    pub version: i64,
}

/// The paired phone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HelloResponseDataDevice {
    pub id: String,
    pub name: String,
}

/// Paired: the token is shown once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: PairResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairResponseData {
    pub token: String,
    pub device_id: String,
    pub version: i64,
}

/// The Mac's clock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PingResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: PingResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PingResponseData {
    /// Milliseconds since 1970.
    pub time: i64,
}

/// All sessions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionsListResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: SessionsListResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionsListResponseData {
    pub sessions: Vec<Session>,
    /// The Claude models a new session may start on now, current ones only, as the Mac's catalog lists them. Absent from an older Mac; the phone then offers its short names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<SessionsListResponseDataModelsItem>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionsListResponseDataModelsItem {
    /// What `modelId` of session.new takes.
    pub id: String,
    /// The name to show.
    pub name: String,
}

/// A page of messages, oldest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatGetResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: ChatGetResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGetResponseData {
    pub messages: Vec<ChatMessage>,
    /// There are older messages (before) or newer ones (after).
    pub has_more: bool,
    /// The after message is gone; replace what the phone has with this page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset: Option<bool>,
}

/// The viz specs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VizGetResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: VizGetResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VizGetResponseData {
    /// The tab to open first: flow, diagram, entity, chart or web.
    pub suggested: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "webURL")]
    pub web_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_candidates: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<VizSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagram: Option<VizSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart: Option<VizSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity: Option<VizSpec>,
}

/// A WEB1 binary frame follows with the same request id; then a second answer, SentResponse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSnapshotResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: WebSnapshotResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSnapshotResponseData {
    pub binary: bool,
    pub bytes: i64,
    pub format: String,
    pub url: String,
}

/// An IMG1 binary frame follows with the same request id; then a second answer, SentResponse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatImageResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: ChatImageResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatImageResponseData {
    pub binary: bool,
    pub bytes: i64,
    pub format: String,
    pub image_id: String,
}

/// Done.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SentResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: SentResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SentResponseData {
    pub sent: bool,
}

/// Done.
pub type SessionMessageResponse = SentResponse;

/// Done.
pub type SessionSendNowResponse = SentResponse;

/// Done.
pub type SessionImageEndResponse = SentResponse;

/// The upload id and the piece size to use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionImageBeginResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: SessionImageBeginResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionImageBeginResponseData {
    pub upload_id: String,
    /// Send pieces of at most this many bytes.
    pub chunk_bytes: i64,
}

/// The piece was stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionImageChunkResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: SessionImageChunkResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionImageChunkResponseData {
    /// The sequence number.
    pub received: i64,
}

/// The slash commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandsListResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: CommandsListResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandsListResponseData {
    pub commands: Vec<Command>,
}

/// The prompt was answered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptAnswerResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: PromptAnswerResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptAnswerResponseData {
    pub answered: bool,
}

/// The session is starting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionNewResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: SessionNewResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionNewResponseData {
    pub started: bool,
}

/// The session to switch to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionTakeoverResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: SessionTakeoverResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTakeoverResponseData {
    /// The new session's id; a pty: id while it is still starting.
    pub session_id: String,
    pub resumed: bool,
    /// The pid of the original claude process that was stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped_pid: Option<i64>,
}

/// Recent folders.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectsRecentResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: ProjectsRecentResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectsRecentResponseData {
    pub projects: Vec<String>,
}

/// Subscribed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscribeResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: SubscribeResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscribeResponseData {
    pub subscribed: bool,
}

/// Unsubscribed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnsubscribeResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: UnsubscribeResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnsubscribeResponseData {
    pub subscribed: bool,
}

/// The inbox as the Mac keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxListResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: InboxListResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxListResponseData {
    pub items: Vec<InboxItem>,
    /// Open items not seen yet.
    pub unread: i64,
}

/// Whether this answer was the one taken.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxAnswerResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// The answer.
    pub data: InboxAnswerResponseData,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxAnswerResponseData {
    /// False when the item was already answered, expired or withdrawn.
    pub answered: bool,
    pub state: String,
    /// The action that answered the item, when one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
}

/// The Mac refused a request, or could not read the frame (then there is no id).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub ok: bool,
    /// What went wrong.
    pub error: ErrorResponseError,
}

/// What went wrong.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorResponseError {
    pub code: ErrorCode,
    /// Text for the person, in English. Never parse it.
    pub message: String,
}

/// The session list changed (also sent once on subscribe).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionsEvent {
    pub event: String,
    pub data: Vec<Session>,
}

/// A new question or permission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptEvent {
    pub event: String,
    pub session_id: String,
    pub prompt: Prompt,
}

/// A prompt was answered elsewhere or timed out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptResolvedEvent {
    pub event: String,
    pub request_id: String,
    pub resolved: bool,
}

/// New messages in the subscribed session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatEvent {
    pub event: String,
    pub session_id: String,
    pub messages: Vec<ChatMessage>,
}

/// The person chose Open on the Mac: the phone should come to the front.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenEvent {
    pub event: String,
}

/// An inbox item was raised or changed. Sent only to a connection that subscribed with inbox: true.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxEvent {
    pub event: String,
    pub item: InboxItem,
}

/// An inbox item was removed from the Mac's inbox.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxRemovedEvent {
    pub event: String,
    /// The removed item's id.
    pub removed: String,
}

fn round<T: DeserializeOwned + Serialize>(value: &Value) -> Result<Value, serde_json::Error> {
    let typed: T = serde_json::from_value(value.clone())?;
    serde_json::to_value(typed)
}

/// Parses `value` as the type named `name` and writes it back. `None` when the schema has no such type.
pub fn roundtrip(name: &str, value: &Value) -> Option<Result<Value, serde_json::Error>> {
    Some(match name {
        "Subagent" => round::<Subagent>(value),
        "PromptQuestion" => round::<PromptQuestion>(value),
        "Prompt" => round::<Prompt>(value),
        "Takeover" => round::<Takeover>(value),
        "LastMessage" => round::<LastMessage>(value),
        "Limit" => round::<Limit>(value),
        "Facts" => round::<Facts>(value),
        "Session" => round::<Session>(value),
        "Image" => round::<Image>(value),
        "ToolStep" => round::<ToolStep>(value),
        "ChatMessage" => round::<ChatMessage>(value),
        "Command" => round::<Command>(value),
        "VizMessage" => round::<VizMessage>(value),
        "VizFailure" => round::<VizFailure>(value),
        "FlowCall" => round::<FlowCall>(value),
        "FlowTurn" => round::<FlowTurn>(value),
        "FlowFile" => round::<FlowFile>(value),
        "FlowSpec" => round::<FlowSpec>(value),
        "DiagramBlock" => round::<DiagramBlock>(value),
        "DiagramSpec" => round::<DiagramSpec>(value),
        "EntityFile" => round::<EntityFile>(value),
        "EntityTable" => round::<EntityTable>(value),
        "EntityLink" => round::<EntityLink>(value),
        "EntitySpec" => round::<EntitySpec>(value),
        "UsagePoint" => round::<UsagePoint>(value),
        "UsageTotals" => round::<UsageTotals>(value),
        "ChartCount" => round::<ChartCount>(value),
        "ModelCount" => round::<ModelCount>(value),
        "ChartSpec" => round::<ChartSpec>(value),
        "VizSpec" => round::<VizSpec>(value),
        "InboxAction" => round::<InboxAction>(value),
        "InboxItem" => round::<InboxItem>(value),
        "HelloRequest" => round::<HelloRequest>(value),
        "PairRequest" => round::<PairRequest>(value),
        "PingRequest" => round::<PingRequest>(value),
        "SessionsListRequest" => round::<SessionsListRequest>(value),
        "ChatGetRequest" => round::<ChatGetRequest>(value),
        "VizGetRequest" => round::<VizGetRequest>(value),
        "WebSnapshotRequest" => round::<WebSnapshotRequest>(value),
        "SessionMessageRequest" => round::<SessionMessageRequest>(value),
        "SessionSendNowRequest" => round::<SessionSendNowRequest>(value),
        "SessionImageBeginRequest" => round::<SessionImageBeginRequest>(value),
        "SessionImageChunkRequest" => round::<SessionImageChunkRequest>(value),
        "SessionImageEndRequest" => round::<SessionImageEndRequest>(value),
        "CommandsListRequest" => round::<CommandsListRequest>(value),
        "PromptAnswerRequest" => round::<PromptAnswerRequest>(value),
        "SessionNewRequest" => round::<SessionNewRequest>(value),
        "ChatImageRequest" => round::<ChatImageRequest>(value),
        "SessionTakeoverRequest" => round::<SessionTakeoverRequest>(value),
        "ProjectsRecentRequest" => round::<ProjectsRecentRequest>(value),
        "SubscribeRequest" => round::<SubscribeRequest>(value),
        "UnsubscribeRequest" => round::<UnsubscribeRequest>(value),
        "InboxListRequest" => round::<InboxListRequest>(value),
        "InboxAnswerRequest" => round::<InboxAnswerRequest>(value),
        "HelloResponse" => round::<HelloResponse>(value),
        "PairResponse" => round::<PairResponse>(value),
        "PingResponse" => round::<PingResponse>(value),
        "SessionsListResponse" => round::<SessionsListResponse>(value),
        "ChatGetResponse" => round::<ChatGetResponse>(value),
        "VizGetResponse" => round::<VizGetResponse>(value),
        "WebSnapshotResponse" => round::<WebSnapshotResponse>(value),
        "ChatImageResponse" => round::<ChatImageResponse>(value),
        "SentResponse" => round::<SentResponse>(value),
        "SessionMessageResponse" => round::<SessionMessageResponse>(value),
        "SessionSendNowResponse" => round::<SessionSendNowResponse>(value),
        "SessionImageEndResponse" => round::<SessionImageEndResponse>(value),
        "SessionImageBeginResponse" => round::<SessionImageBeginResponse>(value),
        "SessionImageChunkResponse" => round::<SessionImageChunkResponse>(value),
        "CommandsListResponse" => round::<CommandsListResponse>(value),
        "PromptAnswerResponse" => round::<PromptAnswerResponse>(value),
        "SessionNewResponse" => round::<SessionNewResponse>(value),
        "SessionTakeoverResponse" => round::<SessionTakeoverResponse>(value),
        "ProjectsRecentResponse" => round::<ProjectsRecentResponse>(value),
        "SubscribeResponse" => round::<SubscribeResponse>(value),
        "UnsubscribeResponse" => round::<UnsubscribeResponse>(value),
        "InboxListResponse" => round::<InboxListResponse>(value),
        "InboxAnswerResponse" => round::<InboxAnswerResponse>(value),
        "ErrorResponse" => round::<ErrorResponse>(value),
        "SessionsEvent" => round::<SessionsEvent>(value),
        "PromptEvent" => round::<PromptEvent>(value),
        "PromptResolvedEvent" => round::<PromptResolvedEvent>(value),
        "ChatEvent" => round::<ChatEvent>(value),
        "OpenEvent" => round::<OpenEvent>(value),
        "InboxEvent" => round::<InboxEvent>(value),
        "InboxRemovedEvent" => round::<InboxRemovedEvent>(value),
        _ => return None,
    })
}
