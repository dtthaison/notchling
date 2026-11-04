import Foundation

/// One step of a session as the Chat window shows it. Built from the transcript's tail by
/// `ConversationReader`; the notch keeps its lighter `TranscriptReader.snapshot`.
struct ChatItem: Identifiable, Equatable {
    enum Kind: Equatable {
        case user(text: String, images: Int)
        case assistant(text: String)
        /// Claude Code keeps only how long it thought; the text is usually redacted.
        case thinking(seconds: Double, text: String)
        case tools([ToolCall])
        /// End of a turn: how long it took and with which model.
        case turnEnd(seconds: Double, model: String?)
        case notice(text: String, symbol: String)
        /// What a slash command printed, such as /cost or /context.
        case commandOutput(text: String)
    }

    let id: String
    var kind: Kind
    var date: Date?
}

struct ToolCall: Identifiable, Equatable {
    enum Kind: Equatable { case shell, read, edit, write, search, web, agent, task, ask, skill, other }

    let id: String
    let name: String
    let kind: Kind
    var title: String
    /// The command, path, query or input, as plain text.
    var input: String
    var output: String?
    var isError = false
    var diff: [DiffLine] = []
    var finished = false

    var added: Int { diff.filter { $0.kind == .add }.count }
    var removed: Int { diff.filter { $0.kind == .remove }.count }
}

struct DiffLine: Equatable {
    enum Kind: Equatable { case add, remove, context, hunk }
    let kind: Kind
    let text: String
    let number: Int?
}

struct TaskEntry: Identifiable, Equatable {
    enum Status: String { case pending, inProgress = "in_progress", completed }
    let id: String
    var subject: String
    var status: Status
}

/// The receipt for a session: where it runs and what it did.
struct SessionFacts: Equatable {
    var branch: String?
    var model: String?
    var inputTokens = 0
    var outputTokens = 0
    /// What the model holds after the latest reply (what /context counts): its last request's input and output.
    var contextTokens = 0
    var turns = 0
    var files: [String] = []
    var commands = 0
}

struct Conversation: Equatable {
    var items: [ChatItem] = []
    var tasks: [TaskEntry] = []
    var facts = SessionFacts()
    /// What it's doing right now: the tool still waiting for a result.
    var activity: String?
    /// When your latest prompt was sent, and the tokens Claude has written since (Claude Code's
    /// "47s · ↓ 2.3k tokens" while it works).
    var turnStart: Date?
    var turnTokens = 0

    var lastUserText: [String] {
        items.suffix(8).compactMap { if case .user(let t, _) = $0.kind { return t } else { return nil } }
    }
}

enum ConversationReader {
    /// Where each transcript was read up to, and the parser's state there (last 4 MB on first read).
    private static let cursors = TranscriptCursors<ConversationParser, Conversation>(tailBytes: 4_000_000, limit: 60) {
        $0.conversation()
    }

    /// Reads only what was appended since the last call.
    static func read(_ path: String) -> Conversation {
        cursors.read(path) ?? Conversation()
    }

    // MARK: Parsing

    /// The chat's items, built one transcript line at a time so a cursor can keep the state between reads.
    struct ConversationParser: TranscriptLineParser {
        private var c = Conversation()
        private var callIndex: [String: (item: Int, call: Int)] = [:]
        /// Message ids whose usage was counted, oldest first, so the set can be kept small.
        private var countedMessages = Set<String>()
        private var countedOrder: [String] = []
        // Output tokens per message in the current turn; a message's lines repeat its usage as it streams.
        private var turnOutput: [String: Int] = [:]
        private var files: [String] = []
        private var serial = 0
        // A turn's thinking shows as one row, at its first thought: where it is, and seconds per message
        // (a message's lines each carry its duration, so the largest counts).
        private var thinkingAt: Int?
        private var thinkingSeconds: [String: Double] = [:]

        init() {}

        /// The model id from /model's confirmation: "Set model to `Sonnet 5.5` and saved as …" or
        /// "Kept model as `Opus 5.5`" become "claude-sonnet-5-5".
        static func modelID(inConfirmation text: String) -> String? {
            guard let range = text.range(of: #"(?:Set model to|Kept model as) `([^`]+)`"#, options: .regularExpression),
                  let open = text[range].firstIndex(of: "`"), let close = text[range].lastIndex(of: "`"), open < close
            else { return nil }
            let name = text[text.index(after: open)..<close].lowercased()
            let slug = name.split(whereSeparator: { $0 == " " || $0 == "." }).joined(separator: "-")
            return slug.isEmpty || slug.contains("default") ? nil : "claude-" + slug
        }

        private mutating func nextID(_ prefix: String) -> String { serial += 1; return "\(prefix)\(serial)" }

        private mutating func append(_ kind: ChatItem.Kind, _ date: Date?) {
            switch kind {
            case .user, .turnEnd, .notice: thinkingAt = nil; thinkingSeconds = [:]
            default: break
            }
            c.items.append(ChatItem(id: nextID("i"), kind: kind, date: date))
        }

        mutating func consume(_ line: Data) {
            parseLine(line)
            trim()
        }

        private mutating func parseLine(_ line: Data) {
            // Screenshots and huge tool output: slow to decode and nothing the chat shows whole.
            if line.count > 400_000 { return }
            guard let obj = (try? JSONSerialization.jsonObject(with: line)) as? [String: Any],
                  let type = obj["type"] as? String,
                  obj["isSidechain"] as? Bool != true else { return }
            let date = (obj["timestamp"] as? String).flatMap(parseDate)
            if let branch = obj["gitBranch"] as? String, !branch.isEmpty, branch != "HEAD" { c.facts.branch = branch }

            switch type {
            case "system":
                switch obj["subtype"] as? String {
                case "turn_duration":
                    let ms = (obj["durationMs"] as? NSNumber)?.doubleValue ?? 0
                    append(.turnEnd(seconds: ms / 1000, model: c.facts.model), date)
                    c.facts.turns += 1
                case "compact_boundary":
                    append(.notice(text: "Conversation compacted", symbol: "rectangle.compress.vertical"), date)
                    if let post = ((obj["compactMetadata"] as? [String: Any])?["postTokens"] as? NSNumber)?.intValue {
                        c.facts.contextTokens = post
                    }
                case "local_command":
                    // Claude Code 2.1.31x logs slash commands and their output here instead of as user lines.
                    if let text = obj["content"] as? String {
                        // /model takes effect now, not at the next reply, so the header shouldn't wait for one.
                        if let id = Self.modelID(inConfirmation: text) { c.facts.model = id }
                        if let kind = userItem(text, images: 0) { append(kind, date) }
                    }
                default: break
                }

            case "assistant":
                guard let message = obj["message"] as? [String: Any],
                      let blocks = message["content"] as? [[String: Any]] else { return }
                if let model = message["model"] as? String, model != "<synthetic>" { c.facts.model = model }
                if let id = message["id"] as? String, let out = ((message["usage"] as? [String: Any])?["output_tokens"] as? NSNumber)?.intValue {
                    turnOutput[id] = max(turnOutput[id] ?? 0, out)
                }
                if let id = message["id"] as? String, !countedMessages.contains(id), let usage = message["usage"] as? [String: Any] {
                    countedMessages.insert(id); countedOrder.append(id)
                    func n(_ k: String) -> Int { (usage[k] as? NSNumber)?.intValue ?? 0 }
                    c.facts.inputTokens += n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens")
                    c.facts.outputTokens += n("output_tokens")
                }
                if let usage = message["usage"] as? [String: Any] {
                    func n(_ k: String) -> Int { (usage[k] as? NSNumber)?.intValue ?? 0 }
                    let held = n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens") + n("output_tokens")
                    // Synthetic messages (errors, interruptions) carry zeros.
                    if held > 0 { c.facts.contextTokens = held }
                }
                for block in blocks {
                    switch block["type"] as? String {
                    case "text":
                        let text = (block["text"] as? String ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
                        guard !text.isEmpty else { continue }
                        if let last = c.items.last, case .assistant(let prior) = last.kind {
                            c.items[c.items.count - 1].kind = .assistant(text: prior + "\n\n" + text)
                        } else {
                            append(.assistant(text: text), date)
                        }
                    case "thinking", "redacted_thinking":
                        let seconds = ((obj["thinkingDurationMs"] as? NSNumber)?.doubleValue ?? 0) / 1000
                        let text = (block["thinking"] as? String ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
                        let key = message["id"] as? String ?? nextID("m")
                        thinkingSeconds[key] = max(thinkingSeconds[key] ?? 0, seconds)
                        let total = thinkingSeconds.values.reduce(0, +)
                        if let at = thinkingAt, case .thinking(_, let prior) = c.items[at].kind {
                            let joined = text.isEmpty ? prior : prior.isEmpty ? text : prior + "\n\n" + text
                            c.items[at].kind = .thinking(seconds: total, text: joined)
                        } else {
                            append(.thinking(seconds: total, text: text), date)
                            thinkingAt = c.items.count - 1
                        }
                    case "tool_use":
                        guard let id = block["id"] as? String else { continue }
                        let name = block["name"] as? String ?? "Tool"
                        let input = block["input"] as? [String: Any] ?? [:]
                        let call = makeCall(id: id, name: name, input: input)
                        if call.kind == .shell { c.facts.commands += 1 }
                        if [.edit, .write].contains(call.kind), let path = input["file_path"] as? String ?? input["notebook_path"] as? String,
                           !files.contains(path) { files.append(path) }
                        if name == "TodoWrite", let todos = input["todos"] as? [[String: Any]] {
                            c.tasks = todos.enumerated().map { i, t in
                                TaskEntry(id: "\(i)", subject: t["content"] as? String ?? "",
                                          status: TaskEntry.Status(rawValue: t["status"] as? String ?? "") ?? .pending)
                            }
                        }
                        if let last = c.items.last, case .tools(var calls) = last.kind {
                            calls.append(call)
                            c.items[c.items.count - 1].kind = .tools(calls)
                            callIndex[id] = (c.items.count - 1, calls.count - 1)
                        } else {
                            append(.tools([call]), date)
                            callIndex[id] = (c.items.count - 1, 0)
                        }
                    default: break
                    }
                }

            case "user":
                guard obj["isMeta"] as? Bool != true, let message = obj["message"] as? [String: Any] else { return }
                if obj["isCompactSummary"] as? Bool == true {
                    // The compact_boundary line just before it already showed this; older builds wrote only the summary.
                    let notice = ChatItem.Kind.notice(text: "Conversation compacted", symbol: "rectangle.compress.vertical")
                    if c.items.last?.kind != notice { append(notice, date) }
                    return
                }
                let result = obj["toolUseResult"] as? [String: Any]
                if let text = message["content"] as? String {
                    if let kind = userItem(text, images: 0) {
                        append(kind, date)
                        if case .user = kind { c.turnStart = date; turnOutput = [:] }
                    }
                    return
                }
                guard let blocks = message["content"] as? [[String: Any]] else { return }
                var text = ""
                var images = 0
                for block in blocks {
                    switch block["type"] as? String {
                    case "tool_result":
                        guard let id = block["tool_use_id"] as? String, let at = callIndex[id],
                              case .tools(var calls) = c.items[at.item].kind else { continue }
                        var call = calls[at.call]
                        call.finished = true
                        call.isError = block["is_error"] as? Bool == true
                        call.output = clip(resultText(block["content"]), result: result)
                        if let patch = result?["structuredPatch"] as? [[String: Any]], !patch.isEmpty {
                            call.diff = diffLines(patch)
                        } else if call.kind == .write, let content = result?["content"] as? String {
                            call.diff = content.split(separator: "\n", omittingEmptySubsequences: false).prefix(60).enumerated()
                                .map { DiffLine(kind: .add, text: String($1), number: $0 + 1) }
                            if result?["type"] as? String == "create" { call.title = call.title.replacingOccurrences(of: "Wrote ", with: "Created ") }
                        }
                        if call.kind == .edit || call.kind == .write { call.output = nil }
                        calls[at.call] = call
                        c.items[at.item].kind = .tools(calls)
                        applyTaskResult(call.name, result: result, input: nil, to: &c)
                    case "text":
                        text += (text.isEmpty ? "" : "\n\n") + (block["text"] as? String ?? "")
                    case "image":
                        images += 1
                    default: break
                    }
                }
                if !text.isEmpty || images > 0, let kind = userItem(text, images: images) {
                    append(kind, date)
                    if case .user = kind { c.turnStart = date; turnOutput = [:] }
                }

            default: break
            }
        }

        /// Keeps a long session bounded: the chat shows the last 500 items, and calls can only finish
        /// in recent ones.
        private mutating func trim() {
            if c.items.count > 1000 {
                let drop = c.items.count - 500
                c.items.removeFirst(drop)
                callIndex = callIndex.compactMapValues { $0.item >= drop ? ($0.item - drop, $0.call) : nil }
                thinkingAt = thinkingAt.flatMap { $0 >= drop ? $0 - drop : nil }
            }
            if countedOrder.count > 4000 {
                countedOrder.prefix(2000).forEach { countedMessages.remove($0) }
                countedOrder.removeFirst(2000)
            }
        }

        /// The conversation as of the lines read so far.
        func conversation() -> Conversation {
            var c = self.c

            // Task subjects come from the TaskCreate inputs; statuses from the results.
            c.facts.files = files
            c.turnTokens = turnOutput.values.reduce(0, +)
            if let last = c.items.last, case .tools(let calls) = last.kind, let open = calls.last(where: { !$0.finished }) {
                c.activity = open.title
            }
            if c.items.count > 500 { c.items = Array(c.items.suffix(500)) }
            return c
        }
    }

    /// What a user line shows as: the text, a slash command as typed, an interruption, or nothing
    /// for injected wrappers.
    private static func userItem(_ raw: String, images: Int) -> ChatItem.Kind? {
        var text = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.hasPrefix("[Request interrupted") {
            return .notice(text: "Interrupted", symbol: "stop.circle")
        }
        if let output = commandOutput(text) { return output.isEmpty ? nil : .commandOutput(text: output) }
        // A "!" shell command typed at the prompt (or sent from Chat), and what it printed.
        if let command = between(text, "<bash-input>", "</bash-input>") {
            return .user(text: "! " + command, images: images)
        }
        if let output = shellOutput(text) { return output.isEmpty ? nil : .commandOutput(text: output) }
        // A slash command shows as what was typed: "/name args".
        if let name = between(text, "<command-name>", "</command-name>") {
            let args = between(text, "<command-args>", "</command-args>") ?? ""
            text = (name.hasPrefix("/") ? name : "/" + name) + (args.isEmpty ? "" : " " + args)
        } else if text.hasPrefix("<") {
            // Injected wrappers: system reminders, command output, hook context.
            return nil
        }
        text = TranscriptReader.collapsePastes(text)
        guard !text.isEmpty || images > 0 else { return nil }
        return .user(text: text, images: images)
    }

    private static func applyTaskResult(_ name: String, result: [String: Any]?, input: [String: Any]?, to c: inout Conversation) {
        guard let result else { return }
        if name == "TaskCreate", let task = result["task"] as? [String: Any], let id = task["id"] as? String {
            let subject = task["subject"] as? String ?? ""
            if let i = c.tasks.firstIndex(where: { $0.id == id }) { c.tasks[i].subject = subject } else {
                c.tasks.append(TaskEntry(id: id, subject: subject, status: .pending))
            }
        } else if name == "TaskUpdate", let id = result["taskId"] as? String,
                  let change = result["statusChange"] as? [String: Any], let to = change["to"] as? String {
            if to == "deleted" { c.tasks.removeAll { $0.id == id }; return }
            if let i = c.tasks.firstIndex(where: { $0.id == id }), let status = TaskEntry.Status(rawValue: to) { c.tasks[i].status = status }
        }
    }

    // MARK: Tools

    private static func makeCall(id: String, name: String, input: [String: Any]) -> ToolCall {
        func str(_ k: String) -> String { input[k] as? String ?? "" }
        func file(_ k: String = "file_path") -> String { (str(k) as NSString).lastPathComponent }
        switch name {
        case "Bash":
            let command = str("command")
            let d = str("description")
            return ToolCall(id: id, name: name, kind: .shell, title: d.isEmpty ? "Ran " + String(command.prefix(60)) : d, input: command)
        case "Read":
            return ToolCall(id: id, name: name, kind: .read, title: "Read " + file(), input: str("file_path"))
        case "Edit", "MultiEdit":
            return ToolCall(id: id, name: name, kind: .edit, title: "Edited " + file(), input: str("file_path"))
        case "NotebookEdit":
            return ToolCall(id: id, name: name, kind: .edit, title: "Edited " + file("notebook_path"), input: str("notebook_path"))
        case "Write":
            return ToolCall(id: id, name: name, kind: .write, title: "Wrote " + file(), input: str("file_path"))
        case "Grep":
            return ToolCall(id: id, name: name, kind: .search, title: "Searched for \(str("pattern"))", input: [str("pattern"), str("path")].filter { !$0.isEmpty }.joined(separator: " in "))
        case "Glob":
            return ToolCall(id: id, name: name, kind: .search, title: "Found files matching \(str("pattern"))", input: str("pattern"))
        case "WebSearch":
            return ToolCall(id: id, name: name, kind: .web, title: "Searched the web for “\(str("query"))”", input: str("query"))
        case "WebFetch":
            let host = URL(string: str("url"))?.host ?? str("url")
            return ToolCall(id: id, name: name, kind: .web, title: "Read \(host)", input: str("url"))
        case "Agent", "Task":
            return ToolCall(id: id, name: name, kind: .agent, title: "Agent: \(str("description").isEmpty ? "subagent" : str("description"))", input: str("prompt"))
        case "Skill":
            return ToolCall(id: id, name: name, kind: .skill, title: "Used the \(str("skill")) skill", input: str("args"))
        case "TaskCreate":
            return ToolCall(id: id, name: name, kind: .task, title: "Added a task: \(str("subject"))", input: str("description"))
        case "TaskUpdate":
            let status = str("status")
            return ToolCall(id: id, name: name, kind: .task, title: status.isEmpty ? "Updated task #\(str("taskId"))" : "Task #\(str("taskId")) \(status.replacingOccurrences(of: "_", with: " "))", input: "")
        case "TodoWrite", "TaskList", "TaskGet":
            return ToolCall(id: id, name: name, kind: .task, title: "Updated the task list", input: "")
        case "AskUserQuestion":
            let q = (input["questions"] as? [[String: Any]])?.first?["question"] as? String ?? ""
            return ToolCall(id: id, name: name, kind: .ask, title: "Asked you: \(q)", input: "")
        default:
            var title = "Used \(name)"
            if name.hasPrefix("mcp__") {
                let parts = name.split(separator: "_", omittingEmptySubsequences: true)
                title = "Used \(parts.last ?? "")" + (parts.count > 2 ? " (\(parts[1]))" : "")
            } else if let action = input["action"] as? String {
                title = "\(name): \(action)" + (input["file_path"].map { " " + (($0 as? String ?? "") as NSString).lastPathComponent } ?? "")
            }
            return ToolCall(id: id, name: name, kind: .other, title: title, input: pretty(input))
        }
    }

    private static func resultText(_ content: Any?) -> String {
        if let s = content as? String { return s }
        if let blocks = content as? [[String: Any]] {
            return blocks.compactMap { $0["type"] as? String == "text" ? $0["text"] as? String : ($0["type"] as? String == "image" ? "[Image]" : nil) }
                .joined(separator: "\n")
        }
        return ""
    }

    /// Shell output from stdout/stderr when there is one, clipped to what a card can show.
    private static func clip(_ text: String, result: [String: Any]?) -> String? {
        var out = text
        if let stdout = result?["stdout"] as? String {
            let stderr = result?["stderr"] as? String ?? ""
            out = [stdout, stderr].filter { !$0.isEmpty }.joined(separator: "\n")
        }
        out = out.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !out.isEmpty else { return nil }
        let lines = out.split(separator: "\n", omittingEmptySubsequences: false)
        if lines.count > 80 { out = lines.prefix(80).joined(separator: "\n") + "\n… \(lines.count - 80) more lines" }
        if out.count > 8000 { out = String(out.prefix(8000)) + "\n…" }
        return out
    }

    private static func diffLines(_ patch: [[String: Any]]) -> [DiffLine] {
        var out: [DiffLine] = []
        for hunk in patch.prefix(12) {
            var old = (hunk["oldStart"] as? NSNumber)?.intValue ?? 0
            var new = (hunk["newStart"] as? NSNumber)?.intValue ?? 0
            out.append(DiffLine(kind: .hunk, text: "@@ line \(new)", number: nil))
            for line in (hunk["lines"] as? [String] ?? []).prefix(120) {
                let body = String(line.dropFirst())
                switch line.first {
                case "+": out.append(DiffLine(kind: .add, text: body, number: new)); new += 1
                case "-": out.append(DiffLine(kind: .remove, text: body, number: old)); old += 1
                default: out.append(DiffLine(kind: .context, text: body, number: new)); old += 1; new += 1
                }
            }
        }
        return out
    }

    private static func pretty(_ input: [String: Any]) -> String {
        guard JSONSerialization.isValidJSONObject(input),
              let data = try? JSONSerialization.data(withJSONObject: input, options: [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]),
              let s = String(data: data, encoding: .utf8) else { return "" }
        return s.count > 4000 ? String(s.prefix(4000)) + "\n…" : s
    }

    // MARK: Helpers

    /// A command's printed output without terminal colors; "" when it printed nothing worth showing,
    /// nil when the text isn't command output.
    private static func commandOutput(_ text: String) -> String? {
        guard let raw = between(text, "<local-command-stdout>", "</local-command-stdout>")
                ?? between(text, "<local-command-stderr>", "</local-command-stderr>") else { return nil }
        let clean = raw.replacingOccurrences(of: "\u{1b}\\[[0-9;?]*[A-Za-z]", with: "", options: .regularExpression)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard clean != "(no content)" else { return "" }
        return clean.count > 6000 ? String(clean.prefix(6000)) + "\n…" : clean
    }

    /// What a "!" command printed: its stdout, then its stderr.
    private static func shellOutput(_ text: String) -> String? {
        let out = between(text, "<bash-stdout>", "</bash-stdout>", trim: false)
        let err = between(text, "<bash-stderr>", "</bash-stderr>", trim: false)
        guard out != nil || err != nil else { return nil }
        let joined = [out, err].compactMap { $0?.trimmingCharacters(in: .newlines) }.filter { !$0.isEmpty }
            .joined(separator: "\n")
        let clean = joined.replacingOccurrences(of: "\u{1b}\\[[0-9;?]*[A-Za-z]", with: "", options: .regularExpression)
        return clean.count > 6000 ? String(clean.prefix(6000)) + "\n…" : clean
    }

    /// The text between two tags. `trim: false` keeps leading spaces, which shell output lines up with.
    private static func between(_ text: String, _ open: String, _ close: String, trim: Bool = true) -> String? {
        guard let a = text.range(of: open), let b = text.range(of: close, range: a.upperBound..<text.endIndex) else { return nil }
        let inner = String(text[a.upperBound..<b.lowerBound])
        return trim ? inner.trimmingCharacters(in: .whitespacesAndNewlines) : inner
    }

    private static let isoFractional: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    private static func parseDate(_ s: String) -> Date? { isoFractional.date(from: s) ?? ISO8601DateFormatter().date(from: s) }

}
