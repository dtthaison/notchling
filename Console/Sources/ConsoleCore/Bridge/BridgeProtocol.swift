import Foundation

public enum BridgeErrorCode: String, Sendable {
    case unauthorized, notFound = "not_found", viewOnly = "view_only", invalid, busy
    case notReachable = "not_reachable", `internal`
}

public struct BridgeFailure: Error, Equatable, Sendable {
    public let code: BridgeErrorCode
    public let message: String

    public init(_ code: BridgeErrorCode, _ message: String) {
        self.code = code
        self.message = message
    }
}

/// JSON on the wire, text frames. Request {"id","op",...}; response {"id","ok":true,"data"} or
/// {"id","ok":false,"error":{"code","message"}}; events {"event",...}.
public enum BridgeWire {
    public static let version = 1

    static func serialize(_ object: Any) -> String {
        guard JSONSerialization.isValidJSONObject(object),
              let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys, .withoutEscapingSlashes]) else {
            return #"{"ok":false,"error":{"code":"internal","message":"Không mã hoá được phản hồi."}}"#
        }
        return String(decoding: data, as: UTF8.self)
    }

    public static func ok(id: Any?, data: Any = [String: Any]()) -> String {
        var object: [String: Any] = ["ok": true, "data": data]
        if let id { object["id"] = id }
        return serialize(object)
    }

    public static func error(id: Any?, _ failure: BridgeFailure) -> String {
        var object: [String: Any] = ["ok": false, "error": ["code": failure.code.rawValue, "message": failure.message]]
        if let id { object["id"] = id }
        return serialize(object)
    }

    public static func event(_ name: String, _ fields: [String: Any]) -> String {
        var object = fields
        object["event"] = name
        return serialize(object)
    }

    /// Any Encodable as a JSON-compatible value (dates become milliseconds via the DTOs).
    public static func object<T: Encodable>(_ value: T) -> Any {
        let encoder = JSONEncoder()
        do {
            return try JSONSerialization.jsonObject(with: try encoder.encode(value), options: [.fragmentsAllowed])
        } catch {
            ConsoleLog.error("BridgeWire.object", "\(T.self)", "\(error)")
            return NSNull()
        }
    }
}

// MARK: - DTOs

public struct PromptDTO: Codable, Equatable, Sendable {
    public struct Question: Codable, Equatable, Sendable {
        public let header: String
        public let question: String
        public let options: [String]
        public let multiSelect: Bool
    }

    public let requestId: String
    public let sessionId: String
    /// "permission" or "question".
    public let kind: String
    public let toolName: String
    public let summary: String
    public let questions: [Question]
    public let created: Double

    public init(_ prompt: PendingPrompt) {
        requestId = prompt.id
        sessionId = prompt.sessionId
        toolName = prompt.toolName
        summary = prompt.summary
        created = prompt.created.timeIntervalSince1970 * 1000
        switch prompt.kind {
        case .permission:
            kind = "permission"
            questions = []
        case .question(let items):
            kind = "question"
            questions = items.map { Question(header: $0.header, question: $0.question, options: $0.options, multiSelect: $0.multiSelect) }
        }
    }
}

public struct SubagentDTO: Codable, Equatable, Sendable {
    public let id: String
    public let type: String
    public let description: String
    public let model: String
    public let depth: Int
    public let finished: Bool
    public let state: String
    public let lastActivity: Double

    public init(_ agent: Subagent) {
        id = agent.id; type = agent.type; description = agent.description; model = agent.model
        depth = agent.depth; finished = agent.finished
        state = agent.state == .running ? "running" : "done"
        lastActivity = agent.lastActivity.timeIntervalSince1970 * 1000
    }
}

public struct SessionDTO: Codable, Equatable, Sendable {
    public let id: String
    public let name: String
    public let project: String
    public let cwd: String
    public let entrypoint: String
    public let state: String
    public let control: String
    public let statusUpdatedAt: Double
    public let startedAt: Double
    public let lastActivity: Double
    public let subagents: [SubagentDTO]
    public let pending: PromptDTO?
    /// {"available":true} or {"available":false,"reason":...}; only on view-only sessions.
    public let takeover: TakeoverDTO?

    public init(_ session: Session) {
        id = session.id; name = session.name; project = session.project; cwd = session.cwd
        switch session.entrypoint {
        case .cli: entrypoint = "cli"
        case .desktop: entrypoint = "desktop"
        case .job: entrypoint = "job"
        case .sdk: entrypoint = "sdk"
        case .other(let raw): entrypoint = raw
        }
        switch session.state {
        case .needsInput: state = "needsInput"
        case .running: state = "running"
        case .done: state = "done"
        case .exited: state = "exited"
        }
        switch session.control {
        case .consolePTY: control = "console"
        case .job: control = "job"
        case .viewOnly: control = "viewOnly"
        }
        statusUpdatedAt = session.statusUpdatedAt.timeIntervalSince1970 * 1000
        startedAt = session.startedAt.timeIntervalSince1970 * 1000
        lastActivity = session.lastActivity.timeIntervalSince1970 * 1000
        subagents = session.subagents.map(SubagentDTO.init)
        pending = session.pending.map(PromptDTO.init)
        switch session.takeover {
        case .available?: takeover = TakeoverDTO(available: true, reason: nil, stoppable: false, busy: false)
        case .stoppable(let busy)?: takeover = TakeoverDTO(available: false, reason: Takeover.aliveReason, stoppable: true, busy: busy)
        case .unavailable(let reason)?: takeover = TakeoverDTO(available: false, reason: reason, stoppable: false, busy: false)
        case nil: takeover = nil
        }
    }
}

public struct TakeoverDTO: Codable, Equatable, Sendable {
    public let available: Bool
    public let reason: String?
    /// The original is still running but Console can stop it (`session.takeover` with `stopOriginal`) and resume.
    public let stoppable: Bool
    /// The original is mid-turn: stopping it interrupts the turn.
    public let busy: Bool
}

/// What `BridgeBackend.takeOver` did: the session to switch to, and the pid of the original it stopped (if any).
public struct TakeoverOutcome: Equatable, Sendable {
    public let sessionId: String
    public let stoppedPid: Int32?

    public init(sessionId: String, stoppedPid: Int32? = nil) {
        self.sessionId = sessionId
        self.stoppedPid = stoppedPid
    }
}

public struct ChatMessageDTO: Codable, Equatable, Sendable {
    public let id: String
    public let role: String
    public let text: String
    public let time: Double
    public let toolSummary: String?
    /// Sent while the agent was working and not yet taken ("đang chờ").
    public let queued: Bool
    /// Descriptions only; the bytes come from `chat.image`.
    public let images: [ImageDTO]

    public init(_ message: ChatMessage) {
        id = message.id.uuidString
        switch message.role {
        case .user: role = "user"
        case .assistant: role = "assistant"
        case .tools: role = "tools"
        }
        text = message.text
        time = message.timestamp.timeIntervalSince1970 * 1000
        toolSummary = message.toolSummary
        queued = message.queued
        images = message.images.map(ImageDTO.init)
    }
}

public struct ImageDTO: Codable, Equatable, Sendable {
    public let id: String
    public let mediaType: String
    public let width: Int?
    public let height: Int?
    public let origin: String
    public let bytes: Int

    public init(_ ref: ImageRef) {
        id = ref.id; mediaType = ref.mediaType; width = ref.width; height = ref.height; origin = ref.origin.rawValue; bytes = ref.byteCount
    }
}

/// How the phone answers a prompt: allow/deny a permission, or pick the answers to a question.
public enum PromptResponse: Equatable, Sendable {
    case allow
    case deny
    case answers([String])
}

public struct PhoneOptions: Equatable, Sendable {
    public var deviceWindowId: UInt32?
    public var fps: Int
    public var width: Int

    public init(deviceWindowId: UInt32? = nil, fps: Int = 10, width: Int = 400) {
        self.deviceWindowId = deviceWindowId
        self.fps = fps
        self.width = width
    }
}

public struct BridgeViz: Sendable {
    public var suggestedTab: String
    public var webURL: String?
    public var specs: [String: VizSpec]

    public init(suggestedTab: String, webURL: String?, specs: [String: VizSpec]) {
        self.suggestedTab = suggestedTab
        self.webURL = webURL
        self.specs = specs
    }
}

/// What the router needs from the app. The live one reads ConsoleModel; tests use a fake.
public protocol BridgeBackend: Sendable {
    func sessions() async -> [Session]
    func chatMessages(sessionId: String, subagentId: String?) async throws -> [ChatMessage]
    /// One page of history: the newest [limit] messages, or the ones before / after a message. The default pages
    /// the newest 200 in memory; the live backend reads the transcript page by page back to its first line.
    func chatPage(sessionId: String, subagentId: String?, before: String?, after: String?, limit: Int) async throws -> BridgeChatPage
    /// A JPEG of a picture the session's chat refers to, at most `maxWidth` wide. Only pictures referenced by that
    /// session's transcript can be asked for. Throws BridgeFailure (not_found, invalid).
    func chatImage(sessionId: String, imageId: String, maxWidth: Int) async throws -> Data
    func viz(sessionId: String, includeEntity: Bool) async throws -> BridgeViz
    func webSnapshot(url: URL, width: Int) async throws -> Data
    func sendMessage(_ text: String, to session: Session) async throws
    func pendingPrompt(requestId: String) async -> PendingPrompt?
    func respond(to prompt: PendingPrompt, with response: PromptResponse) async throws
    func newSession(cwd: URL, model: String?) async throws
    /// Resumes a view-only session whose original process is gone in a Console-owned pty; returns the id of
    /// the session the phone should switch to. Throws BridgeFailure (busy, invalid, notFound).
    /// With [stopOriginal], first stops the original claude process (SIGTERM, verified, 5 s) if one still runs.
    func takeOver(session: Session, stopOriginal: Bool) async throws -> TakeoverOutcome
    func recentProjects() async -> [String]
    /// Shows the Mac's approval sheet and waits for the answer.
    func requestApproval(deviceName: String, platform: String) async -> Bool
    func startPhone(connection: BridgeConnection, options: PhoneOptions) async throws
    func stopPhone(connection: BridgeConnection) async
}

public struct BridgeChatPage: Sendable {
    public var messages: [ChatMessage]
    public var hasMore: Bool
    /// The `after` message is not in the newest window any more: the phone should replace what it has with this page.
    public var reset: Bool

    public init(messages: [ChatMessage], hasMore: Bool, reset: Bool = false) {
        self.messages = messages
        self.hasMore = hasMore
        self.reset = reset
    }
}

extension BridgeBackend {
    public func chatImage(sessionId: String, imageId: String, maxWidth: Int) async throws -> Data {
        throw BridgeFailure(.notFound, "Không thấy ảnh này.")
    }

    public func chatPage(sessionId: String, subagentId: String?, before: String?, after: String?, limit: Int) async throws -> BridgeChatPage {
        var messages = try await chatMessages(sessionId: sessionId, subagentId: subagentId)
        if let before {
            guard let index = messages.firstIndex(where: { $0.id.uuidString == before }) else {
                throw BridgeFailure(.notFound, "Không thấy tin \"before\".")
            }
            messages = Array(messages[..<index])
        } else if let after {
            guard let index = messages.firstIndex(where: { $0.id.uuidString == after }) else {
                let page = messages.suffix(limit)
                return BridgeChatPage(messages: Array(page), hasMore: messages.count > limit, reset: true)
            }
            let newer = Array(messages[(index + 1)...])
            return BridgeChatPage(messages: Array(newer.suffix(limit)), hasMore: newer.count > limit)
        }
        return BridgeChatPage(messages: Array(messages.suffix(limit)), hasMore: messages.count > limit)
    }
}
