import Foundation

public enum SessionState: Int, Sendable, Equatable, CaseIterable {
    case needsInput, running, done, exited

    public var title: String {
        switch self {
        case .needsInput: return "Needs input"
        case .running: return "Running"
        case .done: return "Done"
        case .exited: return "Exited"
        }
    }
}

public enum Entrypoint: Sendable, Equatable {
    case cli, desktop, job, sdk
    case other(String)

    /// Raw values seen in ~/.claude/sessions/<pid>.json.
    public init(raw: String?) {
        switch raw {
        case "cli": self = .cli
        case "claude-desktop": self = .desktop
        case "sdk-cli": self = .sdk
        case let other?: self = .other(other)
        case nil: self = .other("")
        }
    }
}

/// How a message could be sent into the session. Only `.job` is wired up before the PTY launcher exists.
public enum SessionControl: Sendable, Equatable {
    case consolePTY
    case job(String)
    case viewOnly
}

public struct PromptQuestion: Sendable, Equatable {
    public let header: String
    public let question: String
    public let options: [String]
    public let multiSelect: Bool
}

/// A question or permission prompt a Notchling hook is holding open. Read-only here.
public struct PendingPrompt: Sendable, Equatable {
    public enum Kind: Sendable, Equatable {
        case question([PromptQuestion])
        case permission
    }

    public let id: String
    public let sessionId: String
    public let kind: Kind
    public let toolName: String
    public let summary: String
    public let created: Date

    public init(id: String, sessionId: String, kind: Kind, toolName: String, summary: String, created: Date) {
        self.id = id
        self.sessionId = sessionId
        self.kind = kind
        self.toolName = toolName
        self.summary = summary
        self.created = created
    }
}

public enum SubagentState: Sendable, Equatable {
    case running, done
}

public struct Subagent: Identifiable, Sendable, Equatable {
    public let id: String
    public let type: String
    public let description: String
    public let model: String
    public let depth: Int
    public let parentToolUseId: String?
    public let finished: Bool
    public let lastActivity: Date
    public let transcriptURL: URL
    public let state: SubagentState
}

public struct Session: Identifiable, Sendable, Equatable {
    public let id: String
    public var pid: Int32?
    public var name: String
    /// Last path component of `cwd`.
    public var project: String
    public var cwd: String
    public var entrypoint: Entrypoint
    public var kind: String
    public var state: SessionState
    public var statusUpdatedAt: Date
    public var startedAt: Date
    public var transcriptURL: URL?
    public var subagents: [Subagent] = []
    public var pending: PendingPrompt?
    public var control: SessionControl
    /// For view-only sessions on the bridge: whether Console can take it over, and why not.
    public var takeover: TakeoverState?
    /// What the session is doing right now ("Editing main.swift"). Filled in by the app for the phone.
    public var activity: String?
    /// Context size and account limits, for the phone's chat header. Filled in by the app.
    public var facts: SessionFactsInfo?
    /// The newest thing Claude said in this session, so the phone can tell a new reply from an old one without
    /// reading the chat.
    public var lastMessage: LastMessageInfo?
    /// Which tool runs the session: "claude" (the default), "codex" or "agy". Sent to the phone as `tool`.
    public var tool: String = "claude"

    public init(id: String, pid: Int32?, name: String, project: String, cwd: String, entrypoint: Entrypoint, kind: String,
                state: SessionState, statusUpdatedAt: Date, startedAt: Date, transcriptURL: URL?,
                subagents: [Subagent] = [], pending: PendingPrompt? = nil, control: SessionControl) {
        self.id = id; self.pid = pid; self.name = name; self.project = project; self.cwd = cwd
        self.entrypoint = entrypoint; self.kind = kind; self.state = state; self.statusUpdatedAt = statusUpdatedAt
        self.startedAt = startedAt; self.transcriptURL = transcriptURL; self.subagents = subagents
        self.pending = pending; self.control = control
    }

    /// Newest sign of life, including subagents (their transcripts keep moving while the parent is quiet).
    public var lastActivity: Date {
        subagents.reduce(statusUpdatedAt) { max($0, $1.lastActivity) }
    }
}

/// A cheap marker of Claude's newest reply: its id (changes with every new reply) and its first lines.
public struct LastMessageInfo: Sendable, Equatable {
    public var id: String
    public var text: String

    public init(id: String, text: String) { self.id = id; self.text = text }
}

/// One limit of the account a session runs on, as a percentage used.
public struct LimitInfo: Sendable, Equatable {
    public var label: String
    public var percent: Double
    public var resetsAt: Date?

    public init(label: String, percent: Double, resetsAt: Date? = nil) {
        self.label = label; self.percent = percent; self.resetsAt = resetsAt
    }
}

/// The context in use, the model's window (nil when unknown) and the account's limits.
public struct SessionFactsInfo: Sendable, Equatable {
    public var contextTokens: Int?
    public var contextWindow: Int?
    public var model: String?
    public var limits: [LimitInfo]

    public init(contextTokens: Int? = nil, contextWindow: Int? = nil, model: String? = nil, limits: [LimitInfo] = []) {
        self.contextTokens = contextTokens; self.contextWindow = contextWindow; self.model = model; self.limits = limits
    }
}

/// A normal message, a `!` shell command (text is the command, `output` what it printed), or the notice that the
/// conversation was compacted.
public enum ChatKind: String, Sendable, Equatable {
    case text, shell, compaction
}

/// One line of a diff.
public struct DiffRow: Sendable, Equatable {
    public enum Kind: Character, Sendable { case context = " ", added = "+", removed = "-" }
    public var kind: Kind
    public var text: String

    public init(_ kind: Kind, _ text: String) { self.kind = kind; self.text = text }

    /// "+added", "-removed", " context": one string per row on the wire.
    public var wire: String { String(kind.rawValue) + text }
}

/// One tool call: its name, a one-line summary, what it printed, and for Edit/Write the diff.
public struct ToolStep: Sendable, Equatable {
    public var id: String
    public var name: String
    public var summary: String
    public var output: String?
    public var path: String?
    public var diff: [DiffRow]

    public init(id: String, name: String, summary: String, output: String? = nil, path: String? = nil, diff: [DiffRow] = []) {
        self.id = id; self.name = name; self.summary = summary; self.output = output; self.path = path; self.diff = diff
    }
}

public enum ChatRole: Sendable, Equatable {
    case user, assistant, tools
}

public struct ChatMessage: Identifiable, Sendable, Equatable {
    public let id: UUID
    public let role: ChatRole
    public let text: String
    public let timestamp: Date
    /// Set on `.tools` messages: "Read 2 files, ran 1 shell command".
    public let toolSummary: String?
    /// A message the user sent while the agent was working: shown at once, waiting for the agent to take it.
    public var queued: Bool = false
    /// Where this message's first line starts in the transcript file. Lets the history be read page by page
    /// and the newest part be re-read from where it changed. Not sent to phones.
    public var sourceOffset: UInt64? = nil
    /// Pictures in this message (a pasted screenshot, a tool's screenshot, a file named in the text).
    public var images: [ImageRef] = []
    /// Audio files an assistant message names (ElevenLabs voices, sound effects).
    public var audio: [AudioRef] = []
    public var kind: ChatKind = .text
    /// On `.tools` messages: the calls with their input, output and diff.
    public var steps: [ToolStep] = []
    /// On `.shell` messages: what the command printed.
    public var output: String? = nil
}

extension ChatMessage {
    /// For backends outside this module (the struct's memberwise init is internal).
    public init(id: UUID, role: ChatRole, text: String, timestamp: Date, toolSummary: String? = nil) {
        self.init(id: id, role: role, text: text, timestamp: timestamp, toolSummary: toolSummary, queued: false)
    }
}
