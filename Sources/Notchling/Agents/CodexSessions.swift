import Foundation

/// Codex CLI sessions, for Chat. Codex 0.166 writes each session (a "thread") to
/// `~/.codex/sessions/YYYY/MM/DD/rollout-<time>-<id>.jsonl`; older ones are compressed to `.jsonl.zst`
/// and left out. Lines are `{timestamp, ordinal, type, payload}`: `session_meta` first (id, cwd,
/// originator), `response_item` for what the model saw and said (messages, reasoning, tool calls and
/// their outputs), `event_msg` for turn events (`task_started`, `task_complete`, `token_count`,
/// `turn_aborted`), `turn_context` with the turn's model.
enum CodexSessions {
    static var root: URL { CodexStore.home.appendingPathComponent("sessions") }
    /// Chat lists sessions written to in this window.
    static let recent: TimeInterval = 14 * 86400
    /// A turn that started with no word that it ended counts as running while the file changed this recently.
    static let stale: TimeInterval = 600
    static let idPrefix = "codex:"

    struct Meta {
        var id: String
        var cwd: String
        var title: String?
    }

    /// The Chat row id for a Codex session id.
    static func agentID(_ sessionID: String) -> String { idPrefix + sessionID }

    /// Recent sessions as Chat rows, newest first. `running` are session ids Chat is running a turn
    /// for right now (`CodexRunner`), which count as working whatever the file says.
    static func list(now: Date = Date(), running: Set<String> = []) -> [Agent] {
        let fm = FileManager.default
        var out: [Agent] = []
        for day in 0..<Int(recent / 86400) + 1 {
            let date = now.addingTimeInterval(-Double(day) * 86400)
            let parts = Calendar.current.dateComponents([.year, .month, .day], from: date)
            guard let y = parts.year, let m = parts.month, let d = parts.day else { continue }
            let dir = root.appendingPathComponent(String(format: "%04d/%02d/%02d", y, m, d))
            guard let names = try? fm.contentsOfDirectory(atPath: dir.path) else { continue }
            for name in names where name.hasPrefix("rollout-") && name.hasSuffix(".jsonl") {
                let path = dir.appendingPathComponent(name).path
                guard let modified = (try? fm.attributesOfItem(atPath: path))?[.modificationDate] as? Date,
                      now.timeIntervalSince(modified) < recent,
                      let meta = meta(path) else { continue }
                let working = running.contains(meta.id) || (now.timeIntervalSince(modified) < stale && turnOpen(path))
                let folder = (meta.cwd as NSString).lastPathComponent
                out.append(Agent(id: agentID(meta.id), name: meta.title ?? "Codex Session", project: folder, cwd: meta.cwd,
                                 group: working ? .working : .completed, label: working ? "Working" : "Codex",
                                 detail: nil, since: modified, target: .codex(sessionID: meta.id),
                                 transcriptPath: path, host: "Codex"))
            }
        }
        // The same session can't be in two day folders, but be safe.
        var seen = Set<String>()
        return out.sorted { $0.since > $1.since }.filter { seen.insert($0.id).inserted }
    }

    // MARK: Reading a session file

    private static let lock = NSLock()
    private static var metaCache: [String: Meta] = [:]

    /// The session's id and folder from its first line, and its title from the first thing you typed
    /// (read once; the head of a session doesn't change).
    static func meta(_ path: String) -> Meta? {
        lock.lock()
        if let hit = metaCache[path], hit.title != nil { lock.unlock(); return hit }
        lock.unlock()
        guard let handle = FileHandle(forReadingAtPath: path) else { return nil }
        defer { try? handle.close() }
        // The developer prompt and tool list come first and run to a few hundred KB.
        let head = handle.readData(ofLength: 3_000_000)
        var meta: Meta?
        for line in head.split(separator: UInt8(ascii: "\n")) {
            guard let obj = (try? JSONSerialization.jsonObject(with: Data(line))) as? [String: Any],
                  let payload = obj["payload"] as? [String: Any] else { continue }
            let type = obj["type"] as? String
            if type == "session_meta", let id = payload["id"] as? String {
                meta = Meta(id: id, cwd: payload["cwd"] as? String ?? "", title: nil)
            } else if meta != nil, type == "response_item", payload["type"] as? String == "message",
                      payload["role"] as? String == "user", let text = userText(payload) {
                meta?.title = title(text)
                break
            }
        }
        guard let meta else { return nil }
        lock.lock(); metaCache[path] = meta; lock.unlock()
        return meta
    }

    /// The last turn started and hasn't ended, from the file's tail.
    static func turnOpen(_ path: String) -> Bool {
        guard let handle = FileHandle(forReadingAtPath: path) else { return false }
        defer { try? handle.close() }
        let size = (try? handle.seekToEnd()) ?? 0
        try? handle.seek(toOffset: size > 262_144 ? size - 262_144 : 0)
        guard let tail = try? handle.readToEnd() else { return false }
        func last(_ word: String) -> Int { tail.lastRange(of: Data("\"type\":\"\(word)\"".utf8))?.lowerBound ?? -1 }
        let started = last("task_started")
        return started >= 0 && started > max(last("task_complete"), last("turn_aborted"))
    }

    /// What you typed, leaving out what Codex adds as user messages (`<environment_context>`, AGENTS.md).
    static func userText(_ payload: [String: Any]) -> String? {
        let parts = (payload["content"] as? [[String: Any]] ?? []).compactMap { part -> String? in
            guard part["type"] as? String == "input_text" else { return nil }
            return part["text"] as? String
        }
        let text = parts.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, !text.hasPrefix("<"), !text.hasPrefix("# AGENTS.md") else { return nil }
        return text
    }

    static func title(_ text: String) -> String {
        let line = text.split(whereSeparator: \.isNewline).first.map(String.init) ?? text
        return line.count > 80 ? String(line.prefix(79)) + "…" : line
    }
}

/// A Codex session file as Chat's conversation, read incrementally like a Claude transcript.
enum CodexReader {
    private static let cursors = TranscriptCursors<Parser, Conversation>(tailBytes: 4_000_000, limit: 30) {
        $0.conversation()
    }

    static func read(_ path: String) -> Conversation {
        cursors.read(path) ?? Conversation()
    }

    struct Parser: TranscriptLineParser {
        private var items: [ChatItem] = []
        private var facts = SessionFacts()
        private var turnStart: Date?
        private var model: String?
        /// Tool calls waiting for their output, by call id: where they sit in `items`.
        private var openCalls: [String: (item: Int, call: Int)] = [:]
        private var files = Set<String>()

        init() {}

        func conversation() -> Conversation {
            var c = Conversation()
            c.items = items
            c.facts = facts
            c.facts.files = files.sorted()
            c.turnStart = turnStart
            if turnStart != nil, let open = openCalls.values.max(by: { $0.item < $1.item }),
               case .tools(let calls) = items[open.item].kind {
                c.activity = calls[open.call].title
            }
            return c
        }

        mutating func consume(_ line: Data) {
            guard let obj = (try? JSONSerialization.jsonObject(with: line)) as? [String: Any],
                  let payload = obj["payload"] as? [String: Any] else { return }
            let date = (obj["timestamp"] as? String).flatMap(Self.date)
            let ordinal = obj["ordinal"].map { "\($0)" } ?? UUID().uuidString
            switch obj["type"] as? String {
            case "turn_context":
                if let m = payload["model"] as? String { model = m; facts.model = m }
                let settings = (payload["collaboration_mode"] as? [String: Any])?["settings"] as? [String: Any]
                facts.effort = settings?["reasoning_effort"] as? String ?? payload["effort"] as? String
            case "response_item":
                item(payload, date: date, ordinal: ordinal)
            case "event_msg":
                event(payload, date: date, ordinal: ordinal)
            default:
                break
            }
        }

        private mutating func item(_ p: [String: Any], date: Date?, ordinal: String) {
            let id = p["id"] as? String ?? "codex-\(ordinal)"
            switch p["type"] as? String {
            case "message":
                switch p["role"] as? String {
                case "user":
                    guard let text = CodexSessions.userText(p) else { return }
                    let images = (p["content"] as? [[String: Any]] ?? []).filter { $0["type"] as? String == "input_image" }.count
                    items.append(ChatItem(id: id, kind: .user(text: text, images: images), date: date))
                case "assistant":
                    let text = (p["content"] as? [[String: Any]] ?? []).compactMap { $0["text"] as? String }
                        .joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !text.isEmpty else { return }
                    items.append(ChatItem(id: id, kind: .assistant(text: text), date: date))
                default:
                    return
                }
            case "reasoning":
                let text = (p["summary"] as? [[String: Any]] ?? []).compactMap { $0["text"] as? String }
                    .joined(separator: "\n\n").trimmingCharacters(in: .whitespacesAndNewlines)
                guard !text.isEmpty else { return }
                items.append(ChatItem(id: id, kind: .thinking(seconds: 0, text: text), date: date))
            case "function_call", "custom_tool_call", "local_shell_call":
                let callID = p["call_id"] as? String ?? id
                let name = p["name"] as? String ?? "shell"
                let raw = p["arguments"] as? String ?? p["input"] as? String ?? ""
                add(Self.call(id: callID, name: name, raw: raw, action: p["action"] as? [String: Any]), date: date)
            case "function_call_output", "custom_tool_call_output":
                guard let callID = p["call_id"] as? String, let at = openCalls.removeValue(forKey: callID),
                      case .tools(var calls) = items[at.item].kind else { return }
                let output = Self.outputText(p["output"])
                calls[at.call].output = output.count > 20_000 ? String(output.prefix(20_000)) + "\n…" : output
                calls[at.call].finished = true
                if let code = output.range(of: #"(?:Process exited with code|exit code:?)\s*(\d+)"#, options: .regularExpression),
                   let n = Int(output[code].split(whereSeparator: { !$0.isNumber }).last ?? ""), n != 0 {
                    calls[at.call].isError = true
                }
                items[at.item].kind = .tools(calls)
            default:
                return
            }
        }

        private mutating func add(_ call: ToolCall, date: Date?) {
            if call.kind == .shell { facts.commands += 1 }
            // An edit's input is the files its patch touches, one per line.
            if call.kind == .edit { files.formUnion(call.input.split(whereSeparator: \.isNewline).map(String.init)) }
            if let last = items.indices.last, case .tools(var calls) = items[last].kind {
                calls.append(call)
                items[last].kind = .tools(calls)
                openCalls[call.id] = (last, calls.count - 1)
            } else {
                items.append(ChatItem(id: "tools-\(call.id)", kind: .tools([call]), date: date))
                openCalls[call.id] = (items.count - 1, 0)
            }
        }

        private mutating func event(_ p: [String: Any], date: Date?, ordinal: String) {
            switch p["type"] as? String {
            case "task_started":
                turnStart = date ?? Date()
            case "task_complete":
                let seconds = (p["duration_ms"] as? Double).map { $0 / 1000 }
                    ?? turnStart.flatMap { start in date.map { $0.timeIntervalSince(start) } } ?? 0
                items.append(ChatItem(id: "end-\(ordinal)", kind: .turnEnd(seconds: seconds, model: model), date: date))
                facts.turns += 1
                turnStart = nil
                openCalls.removeAll()
            case "turn_aborted":
                items.append(ChatItem(id: "stop-\(ordinal)", kind: .notice(text: "Stopped", symbol: "stop.circle"), date: date))
                turnStart = nil
                openCalls.removeAll()
            case "token_count":
                guard let info = p["info"] as? [String: Any] else { return }
                if let total = info["total_token_usage"] as? [String: Any] {
                    facts.inputTokens = total["input_tokens"] as? Int ?? facts.inputTokens
                    facts.outputTokens = total["output_tokens"] as? Int ?? facts.outputTokens
                }
                if let last = info["last_token_usage"] as? [String: Any] {
                    facts.contextTokens = (last["input_tokens"] as? Int ?? 0) + (last["output_tokens"] as? Int ?? 0)
                }
            case "error":
                let text = p["message"] as? String ?? "Codex hit an error."
                items.append(ChatItem(id: "error-\(ordinal)", kind: .notice(text: text, symbol: "exclamationmark.triangle"), date: date))
            default:
                return
            }
        }

        // MARK: Shapes

        /// A Codex tool call as a Chat step: shell commands, patches, anything else by name.
        static func call(id: String, name: String, raw: String, action: [String: Any]?) -> ToolCall {
            let args = (try? JSONSerialization.jsonObject(with: Data(raw.utf8))) as? [String: Any]
            switch name {
            case "shell", "exec_command", "local_shell", "container.exec", "shell_command":
                var command = args?["cmd"] as? String ?? args?["command"] as? String
                if command == nil, let parts = (args?["command"] ?? action?["command"]) as? [String] {
                    // ["bash", "-lc", "the command"]: the script is what matters.
                    command = parts.count == 3 && parts[1] == "-lc" ? parts[2] : parts.joined(separator: " ")
                }
                let text = command ?? raw
                return ToolCall(id: id, name: "Bash", kind: .shell, title: text.split(whereSeparator: \.isNewline).first.map(String.init) ?? "Command",
                                input: text)
            case "apply_patch":
                let patch = args?["input"] as? String ?? raw
                let files = patchFiles(patch)
                let title = files.isEmpty ? "Edit" : "Edit " + files.map { ($0 as NSString).lastPathComponent }.joined(separator: ", ")
                return ToolCall(id: id, name: "Edit", kind: .edit, title: title, input: files.joined(separator: "\n"),
                                diff: diff(patch))
            case "view_image":
                return ToolCall(id: id, name: "Read", kind: .read, title: "View " + ((args?["path"] as? String) ?? "image"), input: raw)
            case "web_search":
                return ToolCall(id: id, name: "WebSearch", kind: .web, title: (args?["query"] as? String) ?? "Web search", input: raw)
            case "update_plan":
                return ToolCall(id: id, name: "TodoWrite", kind: .task, title: "Update plan", input: raw)
            default:
                return ToolCall(id: id, name: name, kind: .other, title: name, input: raw)
            }
        }

        /// The files an `apply_patch` touches (`*** Update File: path`, Add, Delete).
        static func patchFiles(_ patch: String) -> [String] {
            patch.split(whereSeparator: \.isNewline).compactMap { line -> String? in
                for prefix in ["*** Update File: ", "*** Add File: ", "*** Delete File: "] where line.hasPrefix(prefix) {
                    return String(line.dropFirst(prefix.count)).trimmingCharacters(in: .whitespaces)
                }
                return nil
            }
        }

        static func diff(_ patch: String) -> [DiffLine] {
            patch.split(separator: "\n", omittingEmptySubsequences: false).compactMap { line -> DiffLine? in
                if line.hasPrefix("***") { return nil }
                if line.hasPrefix("@@") { return DiffLine(kind: .hunk, text: String(line), number: nil) }
                if line.hasPrefix("+") { return DiffLine(kind: .add, text: String(line.dropFirst()), number: nil) }
                if line.hasPrefix("-") { return DiffLine(kind: .remove, text: String(line.dropFirst()), number: nil) }
                if line.hasPrefix(" ") { return DiffLine(kind: .context, text: String(line.dropFirst()), number: nil) }
                return nil
            }
        }

        static func outputText(_ value: Any?) -> String {
            if let s = value as? String {
                // exec output can be JSON {"output": "...", "metadata": {...}}.
                if let obj = (try? JSONSerialization.jsonObject(with: Data(s.utf8))) as? [String: Any], let out = obj["output"] as? String {
                    return out
                }
                return s
            }
            if let parts = value as? [[String: Any]] {
                return parts.compactMap { $0["text"] as? String }.joined(separator: "\n")
            }
            if let obj = value as? [String: Any] { return obj["content"] as? String ?? obj["output"] as? String ?? "" }
            return ""
        }

        private static let isoFrac: ISO8601DateFormatter = {
            let f = ISO8601DateFormatter()
            f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
            return f
        }()

        static func date(_ s: String) -> Date? { isoFrac.date(from: s) ?? ISO8601DateFormatter().date(from: s) }
    }
}

private extension Data {
    func lastRange(of needle: Data) -> Range<Int>? {
        range(of: needle, options: .backwards).map { $0.lowerBound..<$0.upperBound }
    }
}
