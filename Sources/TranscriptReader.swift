import Foundation

/// One entry of the conversation, as the terminal shows it: a prompt, Claude's text, or a
/// one-line summary of the tools it ran in between ("Read 2 files, ran 1 shell command").
struct ChatLine: Identifiable, Equatable {
    enum Kind: Equatable { case user, assistant, tools }

    let id: Int
    let kind: Kind
    let text: String

    var fromUser: Bool { kind == .user }
}

/// A question the agent asked with AskUserQuestion that hasn't been answered yet.
struct PendingQuestion: Equatable {
    struct Item: Equatable {
        let header: String
        let question: String
        let options: [String]
        /// What each option means, as Claude wrote it; empty when unknown.
        var details: [String] = []
        let multiSelect: Bool
    }

    let toolUseID: String
    let items: [Item]

    /// Answering from the notch covers single-choice questions; multi-select needs the terminal.
    var answerable: Bool { !items.isEmpty && !items.contains { $0.multiSelect } }
}

/// What the notch shows about one agent, read from its transcript.
struct Snapshot: Equatable {
    var lines: [ChatLine] = []
    var lastPrompt: String?
    var activity: String?
    var question: PendingQuestion?

    var lastReply: String? { lines.last { $0.kind == .assistant }?.text }
}

/// Reads the tail of a session's transcript (~/.claude/projects/<project>/<session-id>.jsonl).
enum TranscriptReader {
    private static let projectsDir = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent(".claude/projects")
    private static var pathCache: [String: String] = [:]
    private static var snapshotCache: [String: (modified: Date, size: UInt64, snapshot: Snapshot)] = [:]
    private static let lock = NSLock()

    static func lines(for agent: Agent, limit: Int = 12) -> [ChatLine] {
        Array(snapshot(for: agent).lines.suffix(limit))
    }

    /// Re-parses only when the transcript changed since the last call.
    static func snapshot(for agent: Agent) -> Snapshot {
        guard let path = path(for: agent),
              let attrs = try? FileManager.default.attributesOfItem(atPath: path),
              let modified = attrs[.modificationDate] as? Date,
              let size = (attrs[.size] as? NSNumber)?.uint64Value else { return Snapshot() }

        lock.lock()
        if let cached = snapshotCache[path], cached.modified == modified, cached.size == size {
            lock.unlock()
            return cached.snapshot
        }
        lock.unlock()

        let snapshot = parse(tail(of: path))
        lock.lock()
        snapshotCache[path] = (modified, size, snapshot)
        lock.unlock()
        return snapshot
    }

    private static func path(for agent: Agent) -> String? {
        if let p = agent.transcriptPath, FileManager.default.fileExists(atPath: p) { return p }
        lock.lock(); defer { lock.unlock() }
        if let cached = pathCache[agent.id] { return cached }
        let dirs = (try? FileManager.default.contentsOfDirectory(atPath: projectsDir.path)) ?? []
        for dir in dirs {
            let candidate = projectsDir.appendingPathComponent(dir).appendingPathComponent("\(agent.id).jsonl").path
            if FileManager.default.fileExists(atPath: candidate) {
                pathCache[agent.id] = candidate
                return candidate
            }
        }
        return nil
    }

    /// True once the transcript holds a result for the latest call of `tool` with this input, meaning
    /// it was answered somewhere else (the terminal) while a hook still held it open.
    static func toolFinished(_ tool: String, input: [String: Any], in path: String) -> Bool {
        func key(_ value: Any?) -> Data? {
            value.flatMap { try? JSONSerialization.data(withJSONObject: $0, options: [.sortedKeys]) }
        }
        guard let wanted = key(input) else { return false }
        var call: String?
        var finished = Set<String>()
        for line in tail(of: path, bytes: 500_000) where line.utf8.count <= 100_000 {
            guard let obj = (try? JSONSerialization.jsonObject(with: Data(line.utf8))) as? [String: Any],
                  let content = (obj["message"] as? [String: Any])?["content"] as? [[String: Any]] else { continue }
            for item in content {
                switch item["type"] as? String {
                case "tool_use" where item["name"] as? String == tool && key(item["input"]) == wanted:
                    call = item["id"] as? String
                case "tool_result":
                    if let id = item["tool_use_id"] as? String { finished.insert(id) }
                default: break
                }
            }
        }
        return call.map(finished.contains) ?? false
    }

    /// The id of the transcript's last message when it is the error Claude Code writes after the Mac slept
    /// mid-reply, so nothing has happened since; nil otherwise.
    static func sleepCut(for agent: Agent) -> String? {
        guard let path = path(for: agent) else { return nil }
        for line in tail(of: path, bytes: 200_000).reversed() where line.utf8.count <= 100_000 {
            guard let obj = (try? JSONSerialization.jsonObject(with: Data(line.utf8))) as? [String: Any],
                  let type = obj["type"] as? String, type == "user" || type == "assistant" else { continue }
            guard obj["isApiErrorMessage"] as? Bool == true,
                  obj["streamCutCause"] as? String == "stream_suspended" || line.contains("went to sleep mid-response")
            else { return nil }
            return obj["uuid"] as? String
        }
        return nil
    }

    /// A usage-limit error ending a transcript: the entry's id and time, and what it said.
    struct LimitHit: Equatable {
        let id: String
        let at: Date?
        let text: String
    }

    /// The transcript's last message when it is the account's usage-limit error ("You've hit your session
    /// limit · resets 7pm"), so nothing has happened since; nil otherwise. The provider's own throttle
    /// ("API Error: Request rejected (429) · ... cooling down") is a different `error` and text, and
    /// doesn't count.
    static func limitHit(for agent: Agent) -> LimitHit? {
        guard let path = path(for: agent) else { return nil }
        return limitHit(inFile: path)
    }

    static func limitHit(inFile path: String) -> LimitHit? {
        for line in tail(of: path, bytes: 200_000).reversed() where line.utf8.count <= 100_000 {
            guard let obj = (try? JSONSerialization.jsonObject(with: Data(line.utf8))) as? [String: Any],
                  let type = obj["type"] as? String, type == "user" || type == "assistant" else { continue }
            guard obj["isApiErrorMessage"] as? Bool == true, obj["error"] as? String == "rate_limit",
                  let id = obj["uuid"] as? String else { return nil }
            var text = ""
            let content = (obj["message"] as? [String: Any])?["content"]
            if let s = content as? String {
                text = s
            } else if let blocks = content as? [[String: Any]] {
                text = blocks.compactMap { $0["text"] as? String }.joined(separator: "\n")
            }
            guard text.hasPrefix("You've hit your") || text.hasPrefix("You\u{2019}ve hit your") else { return nil }
            let at = (obj["timestamp"] as? String).flatMap { stamp in
                ISO8601DateFormatter.fractional.date(from: stamp) ?? ISO8601DateFormatter().date(from: stamp)
            }
            return LimitHit(id: id, at: at, text: text)
        }
        return nil
    }

    /// The last few hundred KB, starting at a line boundary.
    private static func tail(of path: String, bytes: UInt64 = 2_000_000) -> [Substring] {
        guard let handle = FileHandle(forReadingAtPath: path) else { return [] }
        defer { try? handle.close() }
        let size = (try? handle.seekToEnd()) ?? 0
        let start = size > bytes ? size - bytes : 0
        try? handle.seek(toOffset: start)
        guard let data = try? handle.readToEnd(), let text = String(data: data, encoding: .utf8) else { return [] }
        var lines = text.split(separator: "\n")
        if start > 0, !lines.isEmpty { lines.removeFirst() }
        return lines
    }

    private static func parse(_ raw: [Substring], limit: Int = 40) -> Snapshot {
        var messages: [(kind: ChatLine.Kind, text: String)] = []
        // Tool calls since the last piece of text, summed up when the next text arrives.
        var tools = ToolTally()
        func flushTools() {
            if let summary = tools.summary { messages.append((.tools, summary)) }
            tools = ToolTally()
        }
        var activity: String?
        var asked: [String: PendingQuestion] = [:]
        var lastAsked: String?
        var answered = Set<String>()

        for line in raw {
            // Screenshots and long tool output: nothing the chat shows, and slow to decode.
            if line.utf8.count > 100_000 { continue }
            guard let obj = (try? JSONSerialization.jsonObject(with: Data(line.utf8))) as? [String: Any],
                  let type = obj["type"] as? String, type == "user" || type == "assistant",
                  obj["isSidechain"] as? Bool != true, obj["isMeta"] as? Bool != true,
                  let message = obj["message"] as? [String: Any] else { continue }

            var text = ""
            if let s = message["content"] as? String {
                text = s
            } else if let blocks = message["content"] as? [[String: Any]] {
                for block in blocks {
                    switch block["type"] as? String {
                    case "text":
                        if let t = block["text"] as? String { text += (text.isEmpty ? "" : "\n\n") + t }
                    case "tool_use":
                        let name = block["name"] as? String ?? ""
                        let input = block["input"] as? [String: Any] ?? [:]
                        activity = describe(tool: name, input: input)
                        tools.add(name, input)
                        if name == "AskUserQuestion", let id = block["id"] as? String,
                           let question = question(id: id, input: input) {
                            asked[id] = question
                            lastAsked = id
                        }
                    case "tool_result":
                        if let id = block["tool_use_id"] as? String { answered.insert(id) }
                    default:
                        break
                    }
                }
            }
            text = collapsePastes(text).trimmingCharacters(in: .whitespacesAndNewlines)
            // Skip injected wrappers like <command-name>, <system-reminder> and interrupt markers.
            if text.isEmpty || text.hasPrefix("<") || text.hasPrefix("[Request interrupted") { continue }
            if type == "assistant" { activity = nil }

            flushTools()
            let kind: ChatLine.Kind = type == "user" ? .user : .assistant
            if let last = messages.last, last.kind == kind {
                messages[messages.count - 1].text += "\n\n" + text
            } else {
                messages.append((kind, text))
            }
        }
        flushTools()

        var snapshot = Snapshot()
        snapshot.lines = messages.suffix(limit).enumerated().map { i, m in
            let text = m.text.count > 3000 ? "…" + String(m.text.suffix(3000)) : m.text
            return ChatLine(id: i, kind: m.kind, text: text)
        }
        snapshot.lastPrompt = messages.last { $0.kind == .user }?.text
        snapshot.activity = activity
        if let id = lastAsked, !answered.contains(id) { snapshot.question = asked[id] }
        return snapshot
    }

    static func question(id: String, input: [String: Any]) -> PendingQuestion? {
        guard let questions = input["questions"] as? [[String: Any]] else { return nil }
        let items = questions.map { q in
            PendingQuestion.Item(
                header: q["header"] as? String ?? "",
                question: q["question"] as? String ?? "",
                options: (q["options"] as? [[String: Any]] ?? []).compactMap { $0["label"] as? String },
                details: (q["options"] as? [[String: Any]] ?? []).map { $0["description"] as? String ?? "" },
                multiSelect: q["multiSelect"] as? Bool ?? false)
        }
        return PendingQuestion(toolUseID: id, items: items)
    }

    /// Counts tool calls the way the terminal collapses them: "Read 2 files, searched for 1 pattern".
    private struct ToolTally {
        private var order: [String] = []
        private var counts: [String: Int] = [:]
        private var files: [String: Set<String>] = [:]

        mutating func add(_ tool: String, _ input: [String: Any]) {
            let key: String
            switch tool {
            case "Read": key = "read"
            case "Edit", "MultiEdit", "NotebookEdit": key = "edit"
            case "Write": key = "write"
            case "Grep", "Glob": key = "search"
            case "Bash": key = TranscriptReader.bashKind(input["command"] as? String ?? "")
            case "WebSearch", "WebFetch": key = "web"
            case "Agent", "Task": key = "agent"
            case "AskUserQuestion": key = "ask"
            case "TodoWrite", "TaskCreate", "TaskUpdate", "TaskList", "TaskGet": key = "tasks"
            default: key = "use:" + (tool.hasPrefix("mcp__") ? String(tool.split(separator: "_").last ?? "") : tool)
            }
            if counts[key] == nil { order.append(key) }
            counts[key, default: 0] += 1
            if let path = input["file_path"] as? String ?? input["notebook_path"] as? String {
                files[key, default: []].insert(path)
            }
        }

        var summary: String? {
            guard !order.isEmpty else { return nil }
            func n(_ key: String, _ one: String, _ many: String) -> String {
                let count = files[key]?.count ?? counts[key] ?? 0
                return "\(count) \(count == 1 ? one : many)"
            }
            let parts = order.map { key -> String in
                switch key {
                case "read": return "read " + n(key, "file", "files")
                case "edit": return "edited " + n(key, "file", "files")
                case "write": return "wrote " + n(key, "file", "files")
                case "search": return "searched for " + n(key, "pattern", "patterns")
                case "bash": return "ran " + n(key, "shell command", "shell commands")
                case "web": return "searched the web"
                case "agent": return "ran " + n(key, "agent", "agents")
                case "ask": return "asked you"
                case "tasks": return "updated tasks"
                default: return "used " + key.dropFirst(4)
                }
            }
            let text = parts.joined(separator: ", ")
            return text.prefix(1).uppercased() + text.dropFirst()
        }
    }

    /// The terminal counts a shell command that only searches or reads as a search or a read.
    fileprivate static func bashKind(_ command: String) -> String {
        let words = command.components(separatedBy: CharacterSet(charactersIn: "&|;\n"))
            .compactMap { $0.split(separator: " ").first.map(String.init) }
            .filter { $0 != "cd" }
        if !words.isEmpty, words.allSatisfy({ ["grep", "rg", "find", "egrep"].contains($0) }) { return "search" }
        if !words.isEmpty, words.allSatisfy({ ["cat", "head", "tail", "sed", "less", "wc"].contains($0) }) { return "read" }
        return "bash"
    }

    /// Text pasted into a prompt shows as "[Pasted text +N lines]", like the terminal shows it.
    private static func collapsePastes(_ text: String) -> String {
        guard text.contains("<pasted_content") else { return text }
        var out = text
        while let open = out.range(of: "<pasted_content"),
              let openEnd = out.range(of: ">", range: open.upperBound..<out.endIndex),
              let close = out.range(of: "</pasted_content", range: openEnd.upperBound..<out.endIndex),
              let closeEnd = out.range(of: ">", range: close.upperBound..<out.endIndex) {
            let body = out[openEnd.upperBound..<close.lowerBound].trimmingCharacters(in: .newlines)
            let count = body.split(separator: "\n", omittingEmptySubsequences: false).count
            out.replaceSubrange(open.lowerBound..<closeEnd.upperBound,
                                with: count > 1 ? "[Pasted text +\(count) lines]" : "[Pasted text]")
        }
        return out
    }

    /// A short "what it's doing now" line, like "Editing main.swift".
    private static func describe(tool: String, input: [String: Any]) -> String {
        func file(_ key: String = "file_path") -> String {
            ((input[key] as? String ?? "") as NSString).lastPathComponent
        }
        switch tool {
        case "Edit", "MultiEdit": return "Editing \(file())"
        case "Write": return "Writing \(file())"
        case "Read": return "Reading \(file())"
        case "NotebookEdit": return "Editing \(file("notebook_path"))"
        case "Bash":
            if let d = input["description"] as? String, !d.isEmpty { return d }
            return "Running \((input["command"] as? String ?? "").prefix(60))"
        case "Grep", "Glob": return "Searching \(input["pattern"] as? String ?? "")"
        case "WebFetch", "WebSearch": return "Searching the web"
        case "Agent", "Task": return "Running \(input["description"] as? String ?? "a subagent")"
        case "AskUserQuestion": return "Asking you a question"
        case "TodoWrite", "TaskCreate", "TaskUpdate": return "Updating its task list"
        default:
            let short = tool.hasPrefix("mcp__") ? String(tool.split(separator: "_").last ?? "") : tool
            return "Using \(short)"
        }
    }
}


private extension ISO8601DateFormatter {
    static let fractional: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()
}
