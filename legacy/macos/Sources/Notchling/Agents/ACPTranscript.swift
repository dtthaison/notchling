import Foundation

/// A conversation built from Agent Client Protocol `session/update` payloads (`params.update`), as
/// Antigravity CLI's `agy acp` sends them: the whole history once on `session/load`, then live while a
/// turn runs. Text arrives in chunks, merged while the same kind keeps coming; tool calls arrive as a
/// `tool_call` and then `tool_call_update`s with the same id. Pure, so it's tested with a recording.
struct ACPTranscript: Equatable {
    private(set) var items: [ChatItem] = []
    private(set) var tasks: [TaskEntry] = []
    /// Antigravity's own title for the conversation (`session_info_update`).
    private(set) var title: String?
    var model: String?
    private(set) var facts = SessionFacts()
    /// When the running turn began; nil between turns.
    private(set) var turnStart: Date?
    /// The kind of the last text or tool update, so chunks of one message join up.
    private var lastKind: String?
    private var counter = 0
    /// A `usage_update` came in this turn, so the context size is known and the turn's totals only add up.
    private var reportedContext = false

    // MARK: Updates

    mutating func apply(_ update: [String: Any], at date: Date? = nil) {
        guard let kind = update["sessionUpdate"] as? String else { return }
        switch kind {
        case "user_message_chunk", "agent_message_chunk", "agent_thought_chunk":
            let text = Self.text(update["content"])
            guard !text.isEmpty else { return }
            // Gemini reports an approval-mode change as a message of its own; the mode shows in the picker.
            if kind == "agent_message_chunk", text.hasPrefix("[MODE_UPDATE]") { return }
            if lastKind == kind, let last = items.indices.last {
                switch items[last].kind {
                case .user(let t, let images): items[last].kind = .user(text: t + text, images: images)
                case .assistant(let t): items[last].kind = .assistant(text: t + text)
                case .thinking(let s, let t): items[last].kind = .thinking(seconds: s, text: t + text)
                default: append(Self.item(kind, text), at: date)
                }
            } else {
                append(Self.item(kind, text), at: date)
            }
            lastKind = kind
        case "tool_call", "tool_call_update":
            guard let id = update["toolCallId"] as? String else { return }
            if let (i, j) = toolIndex(id), case .tools(var calls) = items[i].kind {
                calls[j] = Self.toolCall(update, merging: calls[j])
                items[i].kind = .tools(calls)
            } else if case .tools(var calls)? = items.last?.kind, lastKind == "tool_call" {
                calls.append(Self.toolCall(update, merging: nil))
                items[items.count - 1].kind = .tools(calls)
            } else {
                append(.tools([Self.toolCall(update, merging: nil)]), at: date)
            }
            noteFacts(update)
            lastKind = "tool_call"
        case "plan":
            let entries = update["entries"] as? [[String: Any]] ?? []
            tasks = entries.enumerated().map { i, e in
                let status: TaskEntry.Status = {
                    switch e["status"] as? String {
                    case "completed": return .completed
                    case "in_progress": return .inProgress
                    default: return .pending
                    }
                }()
                return TaskEntry(id: "plan-\(i)", subject: e["content"] as? String ?? "", status: status)
            }
        case "session_info_update":
            if let t = update["title"] as? String, !t.isEmpty { title = t }
        case "usage_update":
            // Gemini: tokens in the context now (`used`) and the model's window (`size`).
            let used = Self.int(update["used"])
            if used > 0 { facts.contextTokens = used; reportedContext = true }
        default:
            break
        }
    }

    /// You sent a prompt: the next user text starts a new message even if the last one was yours too.
    mutating func beginTurn(at date: Date = Date()) {
        turnStart = date
        lastKind = nil
    }

    /// The prompt's answer arrived (`stopReason`, `usage`).
    mutating func endTurn(stopReason: String?, usage: [String: Any]?, at date: Date = Date()) {
        let seconds = turnStart.map { date.timeIntervalSince($0) } ?? 0
        turnStart = nil
        lastKind = nil
        if stopReason == "cancelled" {
            append(.notice(text: "Stopped", symbol: "stop.circle"), at: date)
        } else if stopReason == "refusal" {
            append(.notice(text: "The model refused this request", symbol: "exclamationmark.triangle"), at: date)
        } else if stopReason == "max_tokens" || stopReason == "max_turn_requests" {
            append(.notice(text: "The turn hit its limit", symbol: "exclamationmark.triangle"), at: date)
        }
        append(.turnEnd(seconds: seconds, model: model), at: date)
        facts.turns += 1
        if let usage {
            // Gemini's input count already includes its cached tokens; Antigravity's doesn't.
            let input = Self.int(usage["inputTokens"]) + (reportedContext ? 0 : Self.int(usage["cachedReadTokens"]))
            let output = Self.int(usage["outputTokens"])
            facts.inputTokens += input
            facts.outputTokens += output
            // A slash command Gemini handled itself reports zeros; the context didn't change.
            if !reportedContext, input + output > 0 { facts.contextTokens = input + output }
        }
        reportedContext = false
    }

    /// A turn that never answered (the process died, or the request failed).
    mutating func failTurn(_ message: String, at date: Date = Date()) {
        turnStart = nil
        lastKind = nil
        append(.notice(text: message, symbol: "exclamationmark.triangle"), at: date)
    }

    func conversation() -> Conversation {
        var c = Conversation()
        c.items = items
        c.tasks = tasks
        c.facts = facts
        c.facts.model = model
        c.turnStart = turnStart
        if turnStart != nil {
            for item in items.reversed() {
                if case .tools(let calls) = item.kind, let open = calls.last(where: { !$0.finished }) {
                    c.activity = open.title
                }
                break
            }
        }
        return c
    }

    // MARK: Helpers

    private mutating func append(_ kind: ChatItem.Kind, at date: Date?) {
        counter += 1
        items.append(ChatItem(id: "agy-\(counter)", kind: kind, date: date))
    }

    private func toolIndex(_ id: String) -> (Int, Int)? {
        for i in items.indices.reversed() {
            if case .tools(let calls) = items[i].kind, let j = calls.firstIndex(where: { $0.id == id }) { return (i, j) }
        }
        return nil
    }

    private mutating func noteFacts(_ update: [String: Any]) {
        guard (update["status"] as? String) == "completed" else { return }
        switch update["kind"] as? String {
        case "edit", "delete", "move":
            for path in Self.paths(update) where !facts.files.contains(path) { facts.files.append(path) }
        case "execute":
            facts.commands += 1
        default:
            break
        }
    }

    private static func item(_ kind: String, _ text: String) -> ChatItem.Kind {
        switch kind {
        case "user_message_chunk": return .user(text: text, images: 0)
        case "agent_thought_chunk": return .thinking(seconds: 0, text: text)
        default: return .assistant(text: text)
        }
    }

    static func text(_ content: Any?) -> String {
        guard let c = content as? [String: Any] else { return "" }
        if let t = c["text"] as? String { return t }
        if (c["type"] as? String) == "resource_link", let name = c["name"] as? String { return "[\(name)]" }
        return ""
    }

    private static func int(_ v: Any?) -> Int { (v as? Int) ?? (v as? NSNumber)?.intValue ?? 0 }

    private static func paths(_ u: [String: Any]) -> [String] {
        let fromLocations = (u["locations"] as? [[String: Any]] ?? []).compactMap { $0["path"] as? String }
        let fromDiffs = (u["content"] as? [[String: Any]] ?? []).compactMap { ($0["type"] as? String) == "diff" ? $0["path"] as? String : nil }
        return Array(NSOrderedSet(array: fromLocations + fromDiffs)) as? [String] ?? []
    }

    /// One ACP tool call as Chat's `ToolCall`; an update carries only what changed, so it merges.
    static func toolCall(_ u: [String: Any], merging old: ToolCall?) -> ToolCall {
        let acpKind = u["kind"] as? String
        let content = u["content"] as? [[String: Any]] ?? []
        let diffs = content.filter { ($0["type"] as? String) == "diff" }
        let kind: ToolCall.Kind = {
            switch acpKind {
            case "execute": return .shell
            case "read": return .read
            case "edit":
                let creates = !diffs.isEmpty && diffs.allSatisfy { (($0["oldText"] as? String) ?? "").isEmpty }
                return creates ? .write : .edit
            case "delete", "move": return .write
            case "search": return .search
            case "fetch": return .web
            default: return old?.kind ?? .other
            }
        }()
        var call = old ?? ToolCall(id: u["toolCallId"] as? String ?? UUID().uuidString,
                                   name: u["name"] as? String ?? acpKind ?? "tool", kind: kind,
                                   title: "", input: "")
        if acpKind != nil, old != nil {
            call = ToolCall(id: call.id, name: call.name, kind: kind, title: call.title, input: call.input,
                            output: call.output, isError: call.isError, diff: call.diff, finished: call.finished)
        }
        if let title = u["title"] as? String, !title.isEmpty { call.title = title }
        if let raw = u["rawInput"] as? [String: Any] {
            call.input = input(raw) ?? call.input
        } else if call.input.isEmpty, let path = paths(u).first {
            call.input = path
        }
        if call.title.isEmpty { call.title = call.input.isEmpty ? call.name : call.input }
        let texts = content.compactMap { c -> String? in
            guard (c["type"] as? String) == "content" else { return nil }
            let t = text(c["content"])
            return t.isEmpty ? nil : t
        }
        if !texts.isEmpty {
            call.output = texts.joined(separator: "\n")
        } else if let raw = u["rawOutput"] as? [String: Any], let out = raw["output"] as? String, !out.isEmpty {
            call.output = out
        }
        if !diffs.isEmpty {
            call.diff = diffs.flatMap { d in
                diff(old: d["oldText"] as? String, new: d["newText"] as? String ?? "")
            }
        }
        switch u["status"] as? String {
        case "completed": call.finished = true
        case "failed": call.finished = true; call.isError = true
        case "pending", "in_progress": call.finished = false
        default: break
        }
        return call
    }

    /// The command, path or query a tool call was given, from Antigravity's argument names.
    private static func input(_ raw: [String: Any]) -> String? {
        for key in ["CommandLine", "command", "Command", "AbsolutePath", "Path", "path", "TargetFile", "Query", "query", "Url", "url", "SearchPath"] {
            if let v = raw[key] as? String, !v.isEmpty { return v }
        }
        let shown = raw.filter { $0.key != "toolSummary" && $0.key != "WaitMsBeforeAsync" }
        guard !shown.isEmpty, let data = try? JSONSerialization.data(withJSONObject: shown, options: [.sortedKeys]) else { return nil }
        return String(String(decoding: data, as: UTF8.self).prefix(400))
    }

    /// Old and new text as diff lines: what both share at the start and end is trimmed to three
    /// context lines, the middle shows as removed then added.
    static func diff(old: String?, new: String) -> [DiffLine] {
        func lines(_ s: String) -> [String] {
            var l = s.components(separatedBy: "\n")
            while l.last?.isEmpty == true { l.removeLast() }
            return l
        }
        let a = lines(old ?? ""), b = lines(new)
        var start = 0
        while start < a.count, start < b.count, a[start] == b[start] { start += 1 }
        var endA = a.count, endB = b.count
        while endA > start, endB > start, a[endA - 1] == b[endB - 1] { endA -= 1; endB -= 1 }
        var out: [DiffLine] = []
        for i in max(0, start - 3)..<start { out.append(DiffLine(kind: .context, text: a[i], number: i + 1)) }
        for i in start..<endA { out.append(DiffLine(kind: .remove, text: a[i], number: i + 1)) }
        for i in start..<endB { out.append(DiffLine(kind: .add, text: b[i], number: i + 1)) }
        for i in endA..<min(a.count, endA + 3) { out.append(DiffLine(kind: .context, text: a[i], number: i + 1)) }
        return out
    }
}
