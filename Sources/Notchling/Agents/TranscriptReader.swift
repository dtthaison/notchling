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
    /// Where each transcript was read up to, and the parser's state there.
    private static var cursors: [String: Cursor] = [:]
    /// Held while a cursor reads and parses, so two refreshes never advance the same one.
    private static let cursorLock = NSLock()
    private static let lock = NSLock()

    static func lines(for agent: Agent, limit: Int = 12) -> [ChatLine] {
        Array(snapshot(for: agent).lines.suffix(limit))
    }

    /// Reads only what was appended since the last call.
    static func snapshot(for agent: Agent) -> Snapshot {
        guard let path = path(for: agent) else { return Snapshot() }
        return snapshot(atPath: path)
    }

    /// The first call parses the last 2 MB; later calls parse only the lines appended since. A file
    /// that was replaced, truncated, or grew by more than 2 MB at once is read from its tail again.
    static func snapshot(atPath path: String) -> Snapshot {
        guard let attrs = try? FileManager.default.attributesOfItem(atPath: path),
              let modified = attrs[.modificationDate] as? Date,
              let size = (attrs[.size] as? NSNumber)?.uint64Value else { return Snapshot() }
        let inode = (attrs[.systemFileNumber] as? NSNumber)?.uint64Value ?? 0

        cursorLock.lock(); defer { cursorLock.unlock() }
        if var cursor = cursors[path], cursor.inode == inode, cursor.offset <= size, size - cursor.offset <= tailBytes {
            guard cursor.modified != modified || cursor.size != size else { return cursor.snapshot }
            if size > cursor.offset, let chunk = read(path, from: cursor.offset, to: size) {
                cursor.offset += chunk.consumed
                for line in chunk.lines { cursor.parser.consume(line) }
            }
            cursor.modified = modified
            cursor.size = size
            cursor.snapshot = cursor.parser.snapshot()
            cursors[path] = cursor
            return cursor.snapshot
        }

        let start = size > tailBytes ? size - tailBytes : 0
        guard let chunk = read(path, from: start, to: size, skipFirstLine: start > 0) else { return Snapshot() }
        var parser = SnapshotParser()
        for line in chunk.lines { parser.consume(line) }
        let snapshot = parser.snapshot()
        if cursors.count > 300 { cursors.removeAll() }
        cursors[path] = Cursor(inode: inode, offset: start + chunk.consumed, modified: modified, size: size,
                               parser: parser, snapshot: snapshot)
        return snapshot
    }

    private static let tailBytes: UInt64 = 2_000_000

    private struct Cursor {
        let inode: UInt64
        /// Just past the last complete line parsed; a line still being written is read next time.
        var offset: UInt64
        var modified: Date
        var size: UInt64
        var parser: SnapshotParser
        var snapshot: Snapshot
    }

    /// The complete lines in `start..<end` and how many bytes they span. `skipFirstLine`: `start` may be
    /// mid-line, so everything up to the first newline is dropped.
    private static func read(_ path: String, from start: UInt64, to end: UInt64,
                             skipFirstLine: Bool = false) -> (lines: [Data], consumed: UInt64)? {
        guard let handle = FileHandle(forReadingAtPath: path) else { return nil }
        defer { try? handle.close() }
        try? handle.seek(toOffset: start)
        guard let data = try? handle.read(upToCount: Int(end - start)) else { return ([], 0) }
        let newline = UInt8(ascii: "\n")
        guard let last = data.lastIndex(of: newline) else { return ([], 0) }
        var from = data.startIndex
        if skipFirstLine, let first = data.firstIndex(of: newline) { from = first + 1 }
        let lines = from <= last ? data[from..<last].split(separator: newline, omittingEmptySubsequences: true) : []
        return (lines.map { Data($0) }, UInt64(last - data.startIndex + 1))
    }

    static func path(for agent: Agent) -> String? {
        if let p = agent.transcriptPath, FileManager.default.fileExists(atPath: p) {
            return successor(of: p, name: agent.name) ?? p
        }
        let now = Date()
        lock.lock()
        if let cached = pathCache[agent.id] { lock.unlock(); return cached }
        let lastScan = missedAt[agent.id]
        lock.unlock()

        // Where Claude Code puts it: the project folder named after the working directory.
        let fm = FileManager.default
        let file = "\(agent.id).jsonl"
        let guess = projectsDir.path + "/" + projectFolder(for: agent.cwd) + "/" + file
        var found: String? = fm.fileExists(atPath: guess) ? guess : nil

        // Otherwise every project folder (hundreds of them), at most every 30 s per agent: a session that
        // has no transcript yet, or keeps it elsewhere, would rescan on every refresh.
        if found == nil, lastScan.map({ now.timeIntervalSince($0) >= 30 }) ?? true {
            let dirs = (try? fm.contentsOfDirectory(atPath: projectsDir.path)) ?? []
            found = dirs.lazy.map { projectsDir.path + "/" + $0 + "/" + file }.first { fm.fileExists(atPath: $0) }
            if found == nil { lock.lock(); missedAt[agent.id] = now; lock.unlock() }
        }
        guard let found else { return nil }
        lock.lock()
        pathCache[agent.id] = found
        missedAt[agent.id] = nil
        lock.unlock()
        return found
    }

    private static var missedAt: [String: Date] = [:]

    /// Claude Code's folder name for a working directory: every character other than a letter or digit
    /// becomes "-" ("/Users/me/.claude" → "-Users-me--claude").
    static func projectFolder(for cwd: String) -> String {
        String(cwd.unicodeScalars.map { CharacterSet.alphanumerics.contains($0) && $0.isASCII ? Character($0) : "-" })
    }

    /// The transcript a background job moved to after /clear. Clearing starts a new file, but the job's
    /// state keeps naming the old one until its next link scan; the new file starts with the job's
    /// name and a SessionStart:clear hook. Follows several clears in a row.
    ///
    /// The search lists the whole project folder and reads every file's dates, and it runs for every
    /// agent on every refresh, so its answer is kept until the folder's modification date changes.
    /// Appending to a transcript leaves that date alone; creating a file (a new transcript after
    /// /clear) moves it.
    static func successor(of path: String, name: String) -> String? {
        let dir = (path as NSString).deletingLastPathComponent
        guard let dirModified = (try? FileManager.default.attributesOfItem(atPath: dir))?[.modificationDate] as? Date
        else { return searchSuccessor(of: path, name: name) }
        let key = path + "\n" + name
        lock.lock()
        if let hit = successorCache[key], hit.dirModified == dirModified { lock.unlock(); return hit.result }
        lock.unlock()
        let result = searchSuccessor(of: path, name: name)
        lock.lock()
        if successorCache.count > 500 { successorCache.removeAll() }
        successorCache[key] = (dirModified, result)
        lock.unlock()
        return result
    }

    private static var successorCache: [String: (dirModified: Date, result: String?)] = [:]

    private static func searchSuccessor(of path: String, name: String) -> String? {
        let fm = FileManager.default
        let dir = (path as NSString).deletingLastPathComponent
        guard !name.isEmpty, let nameData = try? JSONSerialization.data(withJSONObject: [name]),
              let quoted = String(data: nameData, encoding: .utf8)?.dropFirst().dropLast() else { return nil }
        let marker = "\"agentName\":\(quoted)"
        func modified(_ p: String) -> Date? { (try? fm.attributesOfItem(atPath: p))?[.modificationDate] as? Date }
        func created(_ p: String) -> Date? { (try? fm.attributesOfItem(atPath: p))?[.creationDate] as? Date }

        var current = path
        var found: String?
        for _ in 0..<4 {
            guard let since = modified(current)?.addingTimeInterval(-10),
                  let files = try? fm.contentsOfDirectory(atPath: dir) else { break }
            let next = files.lazy
                .filter { $0.hasSuffix(".jsonl") }
                .map { (dir as NSString).appendingPathComponent($0) }
                .filter { $0 != current && $0 != path && (created($0) ?? .distantPast) > since }
                .first { candidate in
                    guard let handle = FileHandle(forReadingAtPath: candidate) else { return false }
                    defer { try? handle.close() }
                    let head = String(decoding: handle.readData(ofLength: 16_384), as: UTF8.self)
                    return head.contains(marker) && head.contains("SessionStart:clear")
                }
            guard let next else { break }
            found = next
            current = next
        }
        return found
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

    /// Turns transcript lines into what the notch shows, one line at a time, so a cursor can keep it
    /// between reads. Keeps only what the snapshot needs: the last messages, the tools since the last
    /// text, the current activity and the last question asked.
    struct SnapshotParser {
        private var messages: [(kind: ChatLine.Kind, text: String)] = []
        /// Tool calls since the last piece of text, summed up when the next text arrives.
        private var tools = ToolTally()
        private var activity: String?
        private var lastPrompt: String?
        private var lastQuestion: PendingQuestion?
        private var lastQuestionAnswered = false

        private static let keptMessages = 120
        /// A message is shown as its last 3000 characters, so longer runs are cut down while kept.
        private static let keptBytes = 16_000

        mutating func consume(_ line: Data) {
            // Screenshots and long tool output: nothing the chat shows, and slow to decode.
            if line.count > 100_000 { return }
            guard let obj = (try? JSONSerialization.jsonObject(with: line)) as? [String: Any],
                  let type = obj["type"] as? String, type == "user" || type == "assistant",
                  obj["isSidechain"] as? Bool != true, obj["isMeta"] as? Bool != true,
                  let message = obj["message"] as? [String: Any] else { return }

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
                        activity = TranscriptReader.describe(tool: name, input: input)
                        tools.add(name, input)
                        if name == "AskUserQuestion", let id = block["id"] as? String,
                           let question = TranscriptReader.question(id: id, input: input) {
                            lastQuestion = question
                            lastQuestionAnswered = false
                        }
                    case "tool_result":
                        if let id = block["tool_use_id"] as? String, id == lastQuestion?.toolUseID {
                            lastQuestionAnswered = true
                        }
                    default:
                        break
                    }
                }
            }
            text = TranscriptReader.collapsePastes(text).trimmingCharacters(in: .whitespacesAndNewlines)
            // Skip injected wrappers like <command-name>, <system-reminder> and interrupt markers.
            if text.isEmpty || text.hasPrefix("<") || text.hasPrefix("[Request interrupted") { return }
            if type == "assistant" { activity = nil }

            flushTools()
            let kind: ChatLine.Kind = type == "user" ? .user : .assistant
            if let last = messages.last, last.kind == kind {
                messages[messages.count - 1].text += "\n\n" + text
            } else {
                messages.append((kind, text))
            }
            if kind == .user { lastPrompt = messages[messages.count - 1].text }
            trim()
        }

        private mutating func flushTools() {
            if let summary = tools.summary { messages.append((.tools, summary)) }
            tools = ToolTally()
        }

        private mutating func trim() {
            if messages.count > Self.keptMessages * 2 { messages.removeFirst(messages.count - Self.keptMessages) }
            let i = messages.count - 1
            if messages[i].kind != .user, messages[i].text.utf8.count > Self.keptBytes {
                messages[i].text = String(messages[i].text.suffix(3001))
            }
        }

        /// The snapshot as of the lines read so far. Tools still pending show as the last line, but stay
        /// open so the next lines can add to them.
        func snapshot(limit: Int = 40) -> Snapshot {
            var shown = messages
            if let summary = tools.summary { shown.append((.tools, summary)) }
            var snapshot = Snapshot()
            snapshot.lines = shown.suffix(limit).enumerated().map { i, m in
                let text = m.text.count > 3000 ? "…" + String(m.text.suffix(3000)) : m.text
                return ChatLine(id: i, kind: m.kind, text: text)
            }
            snapshot.lastPrompt = lastPrompt
            snapshot.activity = activity
            if !lastQuestionAnswered { snapshot.question = lastQuestion }
            return snapshot
        }
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
    static func collapsePastes(_ text: String) -> String {
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
    fileprivate static func describe(tool: String, input: [String: Any]) -> String {
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
