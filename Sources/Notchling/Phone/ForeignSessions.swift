import ConsoleCore
import CryptoKit
import Foundation

/// Codex and Antigravity CLI sessions on the phone. The bridge speaks in `ConsoleCore.Session` and `ChatMessage`;
/// this file maps Chat's own rows (`Agent`) and conversations (`Conversation`) onto them, and the pure parts of it
/// (no runner, no disk) are what `ForeignSessionsTests` checks. The phone sees `tool` = "codex" / "agy" on these
/// sessions; their ids are Chat's row ids (`codex:<id>`, `agy:<id>`).
enum ForeignSessions {
    enum Tool: String {
        case codex, agy

        var idPrefix: String { self == .codex ? CodexSessions.idPrefix : "agy:" }
    }

    /// "codex:abc" -> (.codex, "abc"); nil for a Claude Code session id.
    static func parse(_ rowID: String) -> (tool: Tool, sessionID: String)? {
        for tool in [Tool.codex, .agy] where rowID.hasPrefix(tool.idPrefix) {
            let id = String(rowID.dropFirst(tool.idPrefix.count))
            return id.isEmpty ? nil : (tool, id)
        }
        return nil
    }

    // MARK: List

    /// Rows worth showing on the phone: not closed in Chat (`chatClosedSessions`, row id -> when; a row comes back
    /// once used after that), used within `maxAge`, newest first, at most `perTool` of each tool.
    static func visible(_ agents: [Agent], closed: [String: Double], now: Date = Date(),
                        maxAge: TimeInterval = 7 * 86_400, perTool: Int = 30) -> [Agent] {
        var counts: [Tool: Int] = [:]
        var out: [Agent] = []
        for agent in agents.sorted(by: { $0.since > $1.since }) {
            guard let parsed = parse(agent.id), now.timeIntervalSince(agent.since) < maxAge else { continue }
            if let at = closed[agent.id], agent.since.timeIntervalSince1970 <= at { continue }
            guard counts[parsed.tool, default: 0] < perTool else { continue }
            counts[parsed.tool, default: 0] += 1
            out.append(agent)
        }
        return out
    }

    /// A Chat row as a bridge session. `working` is the live answer (a runner has a turn going), which beats the
    /// row's own group; a waiting permission makes it need input.
    static func session(from agent: Agent, working: Bool, pending: PendingPrompt? = nil) -> Session? {
        guard let parsed = parse(agent.id) else { return nil }
        let state: SessionState = pending != nil ? .needsInput : working ? .running : .done
        var session = Session(id: agent.id, pid: nil, name: agent.name, project: agent.project, cwd: agent.cwd,
                              entrypoint: .cli, kind: "interactive", state: state, statusUpdatedAt: agent.since,
                              startedAt: agent.since, transcriptURL: agent.transcriptPath.map { URL(fileURLWithPath: $0) },
                              pending: pending,
                              // The phone treats "job" as a session it can type into; sends are routed by `tool`.
                              control: .job(agent.id))
        session.tool = parsed.tool.rawValue
        return session
    }

    /// The newest assistant text and the context size, for the list and the chat header.
    static func enrich(_ session: inout Session, with conversation: Conversation) {
        if let item = conversation.items.last(where: { if case .assistant = $0.kind { return true } else { return false } }),
           case .assistant(let text) = item.kind {
            session.lastMessage = LastMessageInfo(id: stableUUID("\(session.id)|\(item.id)").uuidString,
                                                  text: String(text.trimmingCharacters(in: .whitespacesAndNewlines).prefix(240)))
        }
        if session.state == .running { session.activity = conversation.activity }
        let facts = conversation.facts
        if facts.contextTokens > 0 || facts.model != nil {
            session.facts = SessionFactsInfo(contextTokens: facts.contextTokens > 0 ? facts.contextTokens : nil,
                                             contextWindow: nil, model: facts.model, limits: [])
        }
    }

    // MARK: Chat

    /// A conversation as the phone's chat messages. Ids come from the session and item ids, so the same item keeps
    /// its id between reads (the hub pushes a message once, by id). `holdTail` drops a last item that may still
    /// grow (an Antigravity reply streams in chunks, and a tool row gains calls), so it goes out whole.
    static func messages(from conversation: Conversation, sessionID: String, fallbackDate: Date, holdTail: Bool) -> [ChatMessage] {
        var items = conversation.items
        if holdTail, let last = items.last {
            switch last.kind {
            case .assistant, .tools, .thinking: items.removeLast()
            default: break
            }
        }
        var out: [ChatMessage] = []
        var lastDate = fallbackDate
        for item in items {
            let date = item.date ?? lastDate
            lastDate = date
            let id = stableUUID("\(sessionID)|\(item.id)")
            switch item.kind {
            case .user(let text, _):
                out.append(ChatMessage(id: id, role: .user, text: text, timestamp: date))
            case .assistant(let text):
                out.append(ChatMessage(id: id, role: .assistant, text: text, timestamp: date))
            case .tools(let calls):
                let summary = toolSummary(calls)
                var message = ChatMessage(id: id, role: .tools, text: summary, timestamp: date, toolSummary: summary)
                message.steps = calls.map(step)
                out.append(message)
            case .notice(let text, _):
                out.append(ChatMessage(id: id, role: .assistant, text: text, timestamp: date))
            case .commandOutput(let text):
                out.append(ChatMessage(id: id, role: .assistant, text: text, timestamp: date))
            case .thinking, .turnEnd:
                continue
            }
        }
        return out
    }

    static func step(_ call: ToolCall) -> ToolStep {
        let rows: [DiffRow] = call.diff.prefix(120).compactMap { line in
            switch line.kind {
            case .add: return DiffRow(.added, line.text)
            case .remove: return DiffRow(.removed, line.text)
            case .context: return DiffRow(.context, line.text)
            case .hunk: return nil
            }
        }
        let output = call.output.map { $0.count > 6_000 ? String($0.prefix(6_000)) + "\n…" : $0 }
        // An edit's input is the files it touches, one per line; the first absolute one is the step's path.
        let path = call.kind == .edit || call.kind == .write || call.kind == .read
            ? call.input.split(whereSeparator: \.isNewline).map(String.init).first { $0.hasPrefix("/") } : nil
        return ToolStep(id: call.id, name: call.name, summary: String(call.title.prefix(200)), output: output, path: path, diff: rows)
    }

    /// "Ran 2 shell commands, edited 1 file", as the Claude Code chat words it.
    static func toolSummary(_ calls: [ToolCall]) -> String {
        var order: [ToolCall.Kind] = []
        var counts: [ToolCall.Kind: Int] = [:]
        for call in calls {
            if counts[call.kind] == nil { order.append(call.kind) }
            counts[call.kind, default: 0] += 1
        }
        func n(_ kind: ToolCall.Kind, _ one: String, _ many: String) -> String {
            let total = counts[kind] ?? 0
            return "\(total) \(total == 1 ? one : many)"
        }
        let parts = order.map { kind -> String in
            switch kind {
            case .shell: return "ran " + n(kind, "shell command", "shell commands")
            case .read: return "read " + n(kind, "file", "files")
            case .edit: return "edited " + n(kind, "file", "files")
            case .write: return "wrote " + n(kind, "file", "files")
            case .search: return "searched for " + n(kind, "pattern", "patterns")
            case .web: return "searched the web"
            case .agent: return "ran " + n(kind, "agent", "agents")
            case .task: return "updated tasks"
            case .ask: return "asked you"
            case .skill: return "used " + n(kind, "skill", "skills")
            case .other: return "used " + n(kind, "tool", "tools")
            }
        }
        let text = parts.joined(separator: ", ")
        return text.prefix(1).uppercased() + text.dropFirst()
    }

    // MARK: Antigravity permissions

    static let promptPrefix = "agy-perm:"

    /// Antigravity's `session/request_permission` in the bridge's permission shape: the phone gets Allow / Deny, which
    /// answer with the permission's own one-time options (Always Allow and the like stay on the Mac).
    static func prompt(for permission: ACPPermission, rowID: String, created: Date) -> PendingPrompt {
        let call = permission.call
        let line = (call.input.isEmpty ? call.title : call.input).split(whereSeparator: \.isNewline).first.map(String.init) ?? call.title
        return PendingPrompt(id: "\(promptPrefix)\(rowID):\(permission.rpcID)", sessionId: rowID, kind: .permission,
                             toolName: call.name, summary: String("\(call.title): \(line)".prefix(300)), created: created)
    }

    /// (row id, rpc id) from a prompt id made by `prompt(for:rowID:created:)`.
    static func parsePrompt(_ id: String) -> (rowID: String, rpcID: String)? {
        guard id.hasPrefix(promptPrefix) else { return nil }
        let rest = id.dropFirst(promptPrefix.count)
        // The row id is "agy:<uuid>": two colons in, the rpc id starts after the second.
        let pieces = rest.split(separator: ":", maxSplits: 2, omittingEmptySubsequences: false)
        guard pieces.count == 3, !pieces[2].isEmpty else { return nil }
        return ("\(pieces[0]):\(pieces[1])", String(pieces[2]))
    }

    /// The option id that answers `response`; nil means cancel the turn (no option of the kind to pick).
    static func option(for response: PromptResponse, in permission: ACPPermission) -> String? {
        switch response {
        case .allow:
            return (permission.options.first { $0.kind == "allow_once" } ?? permission.options.first { $0.allows })?.id
        case .deny:
            return (permission.options.first { $0.kind == "reject_once" } ?? permission.options.first { !$0.allows })?.id
        case .answers:
            return nil
        }
    }

    // MARK: Ids

    private static func stableUUID(_ seed: String) -> UUID {
        let bytes = Array(SHA256.hash(data: Data(seed.utf8)).prefix(16))
        return UUID(uuid: (bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                           bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]))
    }
}

/// Where the phone gets Codex and Antigravity sessions from, and how it drives them: the same readers and runners
/// Chat uses (`CodexSessions`/`CodexReader`/`CodexRunner`, `AntigravitySessions`/`AntigravityRunner`), so a turn
/// started from the phone shows in Chat and the other way round. Rows are listed from disk at most every 3 s.
final class ForeignSessionSource: @unchecked Sendable {
    private let lock = NSLock()
    private var cached: [Agent] = []
    private var cachedAt = Date.distantPast
    private var inflight: Task<[Agent], Never>?
    /// When each permission was first seen, so the prompt's `created` stays the same between list pushes.
    private var promptSeen: [String: Date] = [:]

    // MARK: List

    /// Chat's Codex and Antigravity rows (14 days), re-read from disk when 3 s old.
    private func rows() async -> [Agent] {
        let task: Task<[Agent], Never>
        lock.lock()
        if Date().timeIntervalSince(cachedAt) < 3 {
            let hit = cached
            lock.unlock()
            return hit
        }
        if let running = inflight {
            task = running
        } else {
            let fresh = Task.detached(priority: .utility) { [weak self] () -> [Agent] in
                let list = CodexSessions.list() + (AntigravityRunner.binary() != nil ? AntigravitySessions.list() : [])
                if let self {
                    self.lock.lock(); self.cached = list; self.cachedAt = Date(); self.inflight = nil; self.lock.unlock()
                }
                return list
            }
            inflight = fresh
            task = fresh
        }
        lock.unlock()
        return await task.value
    }

    private func agent(_ rowID: String) async -> Agent? {
        await rows().first { $0.id == rowID }
    }

    private struct Live {
        var codexRunning: Set<String> = []
        var agyWorking: Set<String> = []
        var permissions: [String: ACPPermission] = [:]
        var conversations: [String: Conversation] = [:]
    }

    /// What the runners know right now. Antigravity's runner is main-thread only.
    private func live(for agents: [Agent]) async -> Live {
        let codex = CodexRunner.shared.running
        return await MainActor.run {
            var live = Live(codexRunning: codex)
            let runner = AntigravityRunner.shared
            live.agyWorking = runner.working
            for agent in agents {
                guard case .antigravity(let id) = agent.target, runner.isOpen(id) else { continue }
                if let permission = runner.permission(id) { live.permissions[agent.id] = permission }
                live.conversations[agent.id] = runner.peekConversation(id)
            }
            return live
        }
    }

    func sessions() async -> [Session] {
        let closed = UserDefaults.standard.dictionary(forKey: "chatClosedSessions") as? [String: Double] ?? [:]
        let agents = ForeignSessions.visible(await rows(), closed: closed)
        guard !agents.isEmpty else { return [] }
        let live = await live(for: agents)
        let now = Date()
        return agents.compactMap { agent -> Session? in
            let working: Bool
            var pending: PendingPrompt?
            switch agent.target {
            case .codex(let id):
                working = live.codexRunning.contains(id) || agent.group == .working
            case .antigravity(let id):
                working = live.agyWorking.contains(id)
                if let permission = live.permissions[agent.id] {
                    pending = ForeignSessions.prompt(for: permission, rowID: agent.id, created: firstSeen(agent.id, permission, now))
                }
            default:
                return nil
            }
            guard var session = ForeignSessions.session(from: agent, working: working, pending: pending) else { return nil }
            if case .codex = agent.target, let path = agent.transcriptPath, now.timeIntervalSince(agent.since) < 6 * 3600 {
                ForeignSessions.enrich(&session, with: CodexReader.read(path))
            } else if let conversation = live.conversations[agent.id] {
                ForeignSessions.enrich(&session, with: conversation)
            }
            return session
        }
    }

    private func firstSeen(_ row: String, _ permission: ACPPermission, _ now: Date) -> Date {
        let key = "\(row):\(permission.rpcID)"
        lock.lock(); defer { lock.unlock() }
        if let at = promptSeen[key] { return at }
        if promptSeen.count > 50 { promptSeen = promptSeen.filter { now.timeIntervalSince($0.value) < 3600 } }
        promptSeen[key] = now
        return now
    }

    // MARK: Chat

    /// The session's messages. An Antigravity conversation is loaded by its own `agy acp` process, started on the
    /// first read; with `waitForLoad` the call waits (up to 8 s) for the history to arrive.
    func messages(sessionID rowID: String, waitForLoad: Bool = false) async throws -> [ChatMessage] {
        guard let agent = await agent(rowID) else { throw BridgeFailure(.notFound, "Không có session này.") }
        switch agent.target {
        case .codex(let id):
            guard let path = agent.transcriptPath else { throw BridgeFailure(.notFound, "Chưa tìm thấy transcript.") }
            let conversation = CodexReader.read(path)
            let running = CodexRunner.shared.isRunning(id)
            return ForeignSessions.messages(from: conversation, sessionID: rowID, fallbackDate: agent.since, holdTail: running)
        case .antigravity(let id):
            await MainActor.run { AntigravityRunner.shared.open(id, folder: agent.cwd) }
            if waitForLoad {
                for _ in 0..<50 {
                    let loading = await MainActor.run { AntigravityRunner.shared.isLoading(id) }
                    if !loading { break }
                    try await Task.sleep(nanoseconds: 160_000_000)
                }
            }
            let (conversation, working, error) = await MainActor.run { () -> (Conversation, Bool, String?) in
                let runner = AntigravityRunner.shared
                return (runner.conversation(id) ?? Conversation(), runner.isWorking(id), runner.loadError(id))
            }
            if let error, conversation.items.isEmpty { throw BridgeFailure(.internal, error) }
            return ForeignSessions.messages(from: conversation, sessionID: rowID, fallbackDate: agent.since, holdTail: working)
        default:
            throw BridgeFailure(.notFound, "Không có session này.")
        }
    }

    func commands(sessionID rowID: String) async -> [CommandInfo] {
        guard let parsed = ForeignSessions.parse(rowID), parsed.tool == .agy else { return [] }
        return await MainActor.run {
            AntigravityRunner.shared.commands(parsed.sessionID).map {
                CommandInfo(name: $0.name, description: $0.description, argumentHint: $0.argumentHint)
            }
        }
    }

    // MARK: Driving

    /// The next turn: `codex exec resume` or a `session/prompt` to `agy acp`, as Chat sends it. One turn at a time;
    /// Chat queues the extra messages, the phone is told the session is busy.
    func send(_ text: String, to session: Session) async throws {
        guard let parsed = ForeignSessions.parse(session.id) else { throw BridgeFailure(.notFound, "Không có session này.") }
        let id = parsed.sessionID
        let folder = session.cwd
        let rowID = session.id
        switch parsed.tool {
        case .codex:
            if CodexRunner.shared.isRunning(id) { throw BridgeFailure(.busy, "Codex đang chạy lượt trước, đợi nó xong.") }
            let pick = Self.codexPick(for: rowID)
            try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
                CodexRunner.shared.run(text, session: id, folder: folder, pick: pick, started: { result in
                    switch result {
                    case .success: done.resume()
                    case .failure(let error): done.resume(throwing: BridgeFailure(.internal, error.localizedDescription))
                    }
                }, ended: { _, failure in
                    if let failure { ConsoleLog.error("phone codex turn", rowID, failure) }
                })
            }
        case .agy:
            try await MainActor.run {
                let runner = AntigravityRunner.shared
                if runner.isWorking(id) { throw BridgeFailure(.busy, "Antigravity đang chạy lượt trước, đợi nó xong.") }
                guard AntigravityRunner.binary() != nil else { throw BridgeFailure(.invalid, "Chưa cài Antigravity CLI.") }
                runner.prompt(id, text: text, folder: folder) { failure in
                    if let failure { ConsoleLog.error("phone agy turn", rowID, failure) }
                }
                if let error = runner.loadError(id) { throw BridgeFailure(.internal, error) }
            }
        }
    }

    /// The model and effort picked in Chat for this Codex row (kept in UserDefaults by `ChatModel`).
    static func codexPick(for rowID: String) -> CodexPick {
        guard let data = UserDefaults.standard.data(forKey: "codexPicks"),
              let saved = try? JSONDecoder().decode([String: CodexPick].self, from: data) else { return CodexPick() }
        return saved[rowID] ?? CodexPick()
    }

    // MARK: Permissions

    func pendingPrompt(requestID: String) async -> PendingPrompt? {
        guard let parsed = ForeignSessions.parsePrompt(requestID), let row = ForeignSessions.parse(parsed.rowID) else { return nil }
        let permission = await MainActor.run { AntigravityRunner.shared.permission(row.sessionID) }
        guard let permission, permission.rpcID == parsed.rpcID else { return nil }
        return ForeignSessions.prompt(for: permission, rowID: parsed.rowID, created: firstSeen(parsed.rowID, permission, Date()))
    }

    func respond(to prompt: PendingPrompt, with response: PromptResponse) async throws {
        guard let parsed = ForeignSessions.parsePrompt(prompt.id), let row = ForeignSessions.parse(parsed.rowID) else {
            throw BridgeFailure(.notFound, "Không còn permission này.")
        }
        try await MainActor.run {
            let runner = AntigravityRunner.shared
            guard let permission = runner.permission(row.sessionID), permission.rpcID == parsed.rpcID else {
                throw BridgeFailure(.notFound, "Không còn permission này.")
            }
            let option = ForeignSessions.option(for: response, in: permission)
            runner.answer(row.sessionID, option: option, cancelled: option == nil)
        }
    }
}
