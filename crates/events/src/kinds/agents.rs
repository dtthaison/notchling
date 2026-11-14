use super::choices::{DoneOrFailed, FailWhy, ToolName};
use crate::field::{Count, Millis, Tokens, Tool, Version};
use crate::field_ref::{Account, Device, Folder, Model, Page, Ref, Session};
use crate::spec::{AgentsRead, Durability, Group, Level, Lock, Proc};

log_choice! {
    pub enum ReplyVia {
        Chat = "chat",
        Notch = "notch",
        Phone = "phone",
        Held = "held",
        Agent = "agent",
    }
}

log_choice! {
    pub enum SendResult {
        Sent = "sent",
        Failed = "failed",
    }
}

log_choice! {
    pub enum HeldThen {
        SentAtTurnEnd = "sent_at_turn_end",
        Replaced = "replaced",
        Cancelled = "cancelled",
        SessionGone = "session_gone",
    }
}

log_choice! {
    pub enum StuckGuess {
        NoDequeueSeen = "no_dequeue_seen",
        ClaudeRestarted = "claude_restarted",
        StillWorking = "still_working",
        BashInputUnmatched = "bash_input_unmatched",
    }
}

log_choice! {
    pub enum PromptKind {
        Question = "question",
        Permission = "permission",
        Plan = "plan",
    }
}

log_choice! {
    pub enum AnswerKind {
        AllowOnce = "allow_once",
        AllowAlways = "allow_always",
        Deny = "deny",
        Picked = "option",
        Typed = "typed",
    }
}

log_choice! {
    pub enum AnswerSurface {
        Chat = "chat",
        Notch = "notch",
        Phone = "phone",
    }
}

log_choice! {
    pub enum ControlAction {
        New = "new",
        Stop = "stop",
        Remove = "remove",
        Move = "move",
        MoveAll = "move_all",
        TrustFolder = "trust_folder",
        Interrupt = "interrupt",
        TakeBack = "take_back",
        SlashCommand = "slash_command",
    }
}

log_choice! {
    pub enum HookEvent {
        PermissionRequest = "permission_request",
        PreToolUse = "pre_tool_use",
        PostToolUse = "post_tool_use",
        PostToolUseFailure = "post_tool_use_failure",
        Notification = "notification",
        Stop = "stop",
        SessionStart = "session_start",
    }
}

log_choice! {
    pub enum HookOutcome {
        AskedApp = "asked_app",
        PassedNoApp = "passed_no_app",
        PassedActingOff = "passed_acting_off",
        PassedTooBig = "passed_too_big",
        PassedTimeout = "passed_timeout",
        RefusedNotDescendant = "refused_not_descendant",
    }
}

log_choice! {
    pub enum TurnResult {
        Done = "done",
        Failed = "failed",
        Stopped = "stopped",
    }
}

log_choice! {
    pub enum PageAction {
        Published = "published",
        ReadRedirected = "read_redirected",
    }
}

log_choice! {
    pub enum VersionedTool {
        Claude = "claude",
        Codex = "codex",
        Agy = "agy",
    }
}

kinds! {
    group: Group::Agents,
    SessionReply = "session.reply", kv 1 {
        title: "Messages Sent",
        records: "A message was typed into a session: where it came from, how long it was, and whether it arrived.",
        why: "Shows what was sent to which agent and from where. The text itself is never recorded.",
        default: On,
        lock: Lock::Toggle,
        sinks: AUDIT | ACTIVITY | DIAG,
        durability: Durability::CommitFromAgent,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::Own,
        keep_days: 30,
        fields {
            session: Ref<Session>,
            via: ReplyVia,
            chars: Count,
            images: Count,
            result: SendResult,
            why: Option<FailWhy>,
            waited_ms: Option<Millis>,
        }
    }
    SessionReplyHeld = "session.reply_held", kv 1 {
        title: "Messages Held",
        records: "A message was kept until the turn ended, and what happened to it then.",
        why: "Shows that a message you typed while an agent was working was not lost.",
        default: On,
        lock: Lock::Toggle,
        sinks: ACTIVITY | DIAG,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::Own,
        keep_days: 30,
        fields {
            session: Ref<Session>,
            then: HeldThen,
        }
    }
    SessionQueueStuck = "session.queue_stuck", kv 1 {
        title: "Messages Stuck in Queued",
        records: "A message stayed Queued for over two minutes, and the app's best guess why.",
        why: "Helps find messages that Claude Code never took, such as after it restarted.",
        default: On,
        lock: Lock::Toggle,
        sinks: ACTIVITY | DIAG,
        durability: Durability::Push,
        level: Level::Warn,
        emitters: &[Proc::App],
        agents_read: AgentsRead::Own,
        keep_days: 30,
        fields {
            session: Ref<Session>,
            waited_ms: Millis,
            guess: StuckGuess,
        }
    }
    SessionAnswered = "session.answered", kv 1 {
        title: "Questions and Permissions Answered",
        records: "A question or a permission prompt from an agent was answered: which tool, what answer, and from where.",
        why: "Shows what you or your phone allowed an agent to do.",
        default: On,
        lock: Lock::Toggle,
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::Own,
        keep_days: 30,
        fields {
            session: Ref<Session>,
            prompt: PromptKind,
            tool: Option<Tool>,
            answer: AnswerKind,
            by: AnswerSurface,
            device: Option<Ref<Device>>,
            waited_ms: Option<Millis>,
        }
    }
    SessionControl = "session.control", kv 1 {
        title: "Sessions Started, Stopped or Moved",
        records: "A session was started, stopped, removed, moved or sent a slash command, and how it ended.",
        why: "Shows who started, stopped or moved which session, and under which account.",
        default: On,
        lock: Lock::Toggle,
        sinks: AUDIT | ACTIVITY,
        durability: Durability::CommitFromAgent,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::Own,
        keep_days: 30,
        fields {
            action: ControlAction,
            command: Option<Tool>,
            session: Option<Ref<Session>>,
            folder: Option<Ref<Folder>>,
            account: Option<Ref<Account>>,
            result: DoneOrFailed,
        }
    }
    HookDecided = "hook.decided", kv 1 {
        title: "Prompts From Claude Code",
        records: "Why a prompt was shown in Notchling or left to Claude Code's own terminal.",
        why: "Explains why a permission prompt appeared in Notchling, or why it did not.",
        default: On,
        lock: Lock::Toggle,
        sinks: ACTIVITY | DIAG,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon, Proc::Hook],
        agents_read: AgentsRead::Own,
        keep_days: 30,
        fields {
            session: Option<Ref<Session>>,
            event: HookEvent,
            tool: Option<Tool>,
            outcome: HookOutcome,
            waited_ms: Option<Millis>,
        }
    }
    ToolTurn = "tool.turn", kv 1 {
        title: "Codex and Antigravity Turns",
        records: "A Codex or Antigravity turn finished: the model, how long it took and how many tokens it used.",
        why: "Shows how much each delegated turn cost in time and tokens.",
        default: On,
        lock: Lock::Toggle,
        sinks: ACTIVITY | DIAG,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::Own,
        keep_days: 30,
        fields {
            tool: ToolName,
            session: Ref<Session>,
            model: Option<Ref<Model>>,
            result: TurnResult,
            duration_ms: Millis,
            tokens_in: Tokens,
            tokens_out: Tokens,
        }
    }
    DelegateJob = "delegate.job", kv 1 {
        title: "Jobs Handed to Another Tool",
        records: "An agent handed a job to Codex or Antigravity with notchling delegate, and how it ended.",
        why: "Sent by the command itself, so it is the agent's own account of what it delegated.",
        default: On,
        lock: Lock::Toggle,
        sinks: ACTIVITY | DIAG,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Cli],
        agents_read: AgentsRead::Own,
        keep_days: 30,
        fields {
            tool: ToolName,
            model: Option<Ref<Model>>,
            write: bool,
            result: DoneOrFailed,
            duration_ms: Millis,
        }
    }
    PagesSaved = "pages.saved", kv 1 {
        title: "Artifact Copies",
        records: "A page published with the Artifact tool was saved locally, or a read was redirected to the local copy.",
        why: "Shows which pages Notchling keeps a copy of, and under which account.",
        default: On,
        lock: Lock::Toggle,
        sinks: ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon, Proc::Hook],
        agents_read: AgentsRead::No,
        keep_days: 30,
        fields {
            page: Ref<Page>,
            action: PageAction,
            account: Option<Ref<Account>>,
        }
    }
    ToolVersionSeen = "tool.version_seen", kv 1 {
        title: "Tool Versions",
        records: "The first time a Claude Code, Codex or Antigravity version was seen. The tool is never run to find out.",
        why: "Shows when a tool updated, which helps when a change in behavior needs explaining.",
        default: On,
        lock: Lock::Toggle,
        sinks: ACTIVITY | DIAG,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 30,
        fields {
            tool: VersionedTool,
            version: Version,
        }
    }
}
