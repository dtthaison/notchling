import Foundation

public struct ChatParseResult: Sendable, Equatable {
    public var messages: [ChatMessage]
    /// Lines that were not JSON; each one is logged with its path and position.
    public var malformedLines: Int
    /// `message.model` of the newest assistant line, for the chat header.
    public var model: String? = nil

    public static let empty = ChatParseResult(messages: [], malformedLines: 0)
}

/// Reads the tail of a main or subagent transcript (.jsonl) into chat messages.
public enum ChatTranscript {
    public static let tailBytes: UInt64 = 2_000_000
    /// Screenshots and long tool output: nothing the chat shows, and slow to decode.
    public static let maxLineBytes = 100_000
    public static let maxTextCharacters = 3000
    public static let defaultLimit = 200

    public static func read(url: URL, isMain: Bool, limit: Int = defaultLimit) -> ChatParseResult {
        guard let window = readWindow(url: url, from: nil, to: nil) else { return .empty }
        return parse(sourceLines: window.lines, isMain: isMain, path: url.path, limit: limit)
    }

    /// The last `bytes` of the file as lines, starting at a line boundary. Nil when it can't be read (logged).
    static func tail(of url: URL, bytes: UInt64 = tailBytes) -> [Substring]? {
        readWindow(url: url, from: nil, to: nil, tail: bytes)?.lines.map(\.text)
    }

    /// A run of whole lines from the file with the byte offset each starts at.
    struct SourceLine {
        let offset: UInt64
        let text: Substring
    }

    struct Window {
        /// Where the first (whole) line starts; 0 when the window reaches the start of the file.
        let start: UInt64
        let end: UInt64
        let lines: [SourceLine]
    }

    /// The bytes [from, to) of the file as whole lines. `from` nil means the last `tailBytes`; `to` nil means the end
    /// of the file. A window that starts mid-file drops the line it starts inside. Nil when the file can't be read.
    static func readWindow(url: URL, from: UInt64?, to: UInt64?, tail: UInt64 = tailBytes) -> Window? {
        let handle: FileHandle
        do {
            handle = try FileHandle(forReadingFrom: url)
        } catch {
            ConsoleLog.error("readWindow", url.path, "open failed: \(error)")
            return nil
        }
        defer {
            do { try handle.close() } catch { ConsoleLog.error("readWindow", url.path, "close failed: \(error)") }
        }
        do {
            let size = try handle.seekToEnd()
            let end = min(to ?? size, size)
            let start = from ?? (end > tail ? end - tail : 0)
            guard end > start else { return Window(start: start, end: end, lines: []) }
            // Read one byte earlier to learn whether `start` is the beginning of a line.
            let from = start > 0 ? start - 1 : 0
            try handle.seek(toOffset: from)
            guard var data = try handle.read(upToCount: Int(end - from)) else { return Window(start: start, end: end, lines: []) }
            var alignedStart = true
            var windowStart = start
            if start > 0 {
                alignedStart = data.first == 0x0A
                data = data.dropFirst()
                if !alignedStart {
                    // The window begins inside a line (a long one: a screenshot, say). Back up to where it starts so
                    // the line is read whole instead of being dropped.
                    var cursor = start
                    var prefix = Data()
                    var lineStart: UInt64?
                    while cursor > 0, prefix.count < 64 * 1024 * 1024, lineStart == nil {
                        let chunkStart = cursor > 1 << 20 ? cursor - (1 << 20) : 0
                        try handle.seek(toOffset: chunkStart)
                        guard let chunk = try handle.read(upToCount: Int(cursor - chunkStart)), !chunk.isEmpty else { break }
                        if let newline = chunk.lastIndex(of: 0x0A) {
                            lineStart = chunkStart + UInt64(newline - chunk.startIndex) + 1
                            prefix = Data(chunk[(newline + 1)...]) + prefix
                        } else {
                            prefix = chunk + prefix
                            if chunkStart == 0 { lineStart = 0 }
                        }
                        cursor = chunkStart
                    }
                    if let lineStart {
                        windowStart = lineStart
                        data = prefix + data
                        alignedStart = true
                    }
                }
            }
            return split(Data(data), start: windowStart, end: end, startsOnLine: alignedStart)
        } catch {
            ConsoleLog.error("readWindow", url.path, "read failed: \(error)")
            return nil
        }
    }

    /// Cuts bytes into lines at "\n", keeping each line's file offset. A window that began mid-file drops its first
    /// (partial) line; a last line without a newline is kept (the writer may still be adding to it, so it is parsed
    /// again later).
    static func split(_ data: Data, start: UInt64, end: UInt64, startsOnLine: Bool = false) -> Window {
        var lines: [SourceLine] = []
        var lineStart = data.startIndex
        var first = true
        func emit(_ range: Range<Data.Index>) {
            defer { first = false }
            if first, start > 0, !startsOnLine { return }
            guard let text = String(data: data[range], encoding: .utf8) else { return }
            lines.append(SourceLine(offset: start + UInt64(range.lowerBound - data.startIndex), text: Substring(text)))
        }
        while let newline = data[lineStart...].firstIndex(of: 0x0A) {
            emit(lineStart..<newline)
            lineStart = newline + 1
        }
        if lineStart < data.endIndex { emit(lineStart..<data.endIndex) }
        let firstOffset = lines.first?.offset ?? start
        return Window(start: firstOffset, end: end, lines: lines)
    }

    static func parse(lines: [Substring], isMain: Bool, path: String, limit: Int = defaultLimit) -> ChatParseResult {
        parse(sourceLines: lines.enumerated().map { SourceLine(offset: UInt64($0.offset), text: $0.element) }, isMain: isMain, path: path, limit: limit)
    }

    static func parse(sourceLines lines: [SourceLine], isMain: Bool, path: String, limit: Int = defaultLimit) -> ChatParseResult {
        struct Draft {
            var role: ChatRole
            var text: String
            var timestamp: Date
            var seed: String
            var toolSummary: String?
            var offset: UInt64
            var queued = false
            var images: [ImageRef] = []
            var audio: [AudioRef] = []
            var kind: ChatKind = .text
            var steps: [ToolStep] = []
            var output: String?
        }
        /// A message the user sent while the agent was busy: waiting until Claude Code takes it.
        struct Waiting {
            var content: String
            var timestamp: Date
            var offset: UInt64
        }

        var drafts: [Draft] = []
        var waiting: [Waiting] = []
        /// Content taken from the queue lately, so the user line that follows it is not shown twice.
        var taken: [(content: String, time: Date)] = []
        var malformed = 0
        var latestModel: String?
        /// Claude Code writes the session's folder on every line; relative audio names are read against it.
        var latestCwd: String?
        var tally = ToolTally()
        var tallyTimestamp = Date.distantPast
        var tallySeed = ""
        var tallyOffset: UInt64 = 0
        var tallyImages: [ImageRef] = []
        /// The calls of the current tool run, and where each call's result goes once it arrives.
        var tallySteps: [ToolStep] = []
        var tallyIndex: [String: Int] = [:]
        var flushedSteps: [String: (draft: Int, step: Int)] = [:]

        func flushTools() {
            if let summary = tally.summary {
                drafts.append(Draft(role: .tools, text: summary, timestamp: tallyTimestamp,
                                    seed: tallySeed + ":tools", toolSummary: summary, offset: tallyOffset, images: tallyImages, steps: tallySteps))
                for (index, step) in tallySteps.enumerated() { flushedSteps[step.id] = (drafts.count - 1, index) }
            } else if !tallyImages.isEmpty, let last = drafts.indices.last {
                // Pictures with no tool row of their own (the row was already written): they go under the last message.
                drafts[last].images += tallyImages
            }
            tallyImages = []
            tallySteps = []
            tallyIndex = [:]
            tally = ToolTally()
        }

        func attachResult(_ id: String, _ output: String) {
            guard !output.isEmpty else { return }
            if let index = tallyIndex[id] {
                tallySteps[index].output = output
            } else if let place = flushedSteps[id], drafts.indices.contains(place.draft), drafts[place.draft].steps.indices.contains(place.step) {
                drafts[place.draft].steps[place.step].output = output
            }
        }

        func addUser(_ text: String, timestamp: Date, seed: String, offset: UInt64, merge: Bool = true, images: [ImageRef] = []) {
            flushTools()
            if merge, let last = drafts.last, last.role == .user, !last.queued, last.kind == .text {
                if !text.isEmpty { drafts[drafts.count - 1].text += (drafts[drafts.count - 1].text.isEmpty ? "" : "\n\n") + text }
                drafts[drafts.count - 1].images += images
            } else {
                drafts.append(Draft(role: .user, text: text, timestamp: timestamp, seed: seed, toolSummary: nil, offset: offset, images: images))
            }
        }

        func wasTaken(_ text: String, near time: Date) -> Bool {
            taken.contains { $0.content == text && abs($0.time.timeIntervalSince(time)) < 120 }
        }

        for (index, source) in lines.enumerated() {
            var line = source.text
            var stripped: [(base64Length: Int, size: (Int, Int)?)] = []
            // A line with pictures is big because of the base64; that is cut out first, and only then is the size judged.
            if line.contains("\"type\":\"base64\"") { (line, stripped) = ChatImages.stripBase64(from: line) }
            if line.utf8.count > maxLineBytes { continue }
            let object: [String: Any]
            do {
                guard let decoded = try JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any] else {
                    malformed += 1
                    ConsoleLog.error("parse", path, "line \(index) is not a JSON object, skipped")
                    continue
                }
                object = decoded
            } catch {
                malformed += 1
                // A last line that is still being written is not an error worth logging loudly.
                ConsoleLog.error("parse", path, "line \(index) is malformed, skipped: \(error)")
                continue
            }
            let timestamp = (object["timestamp"] as? String).flatMap(parseISO) ?? .distantPast
            if let cwd = object["cwd"] as? String, !cwd.isEmpty { latestCwd = cwd }

            // Claude Code's input queue: a message typed while the agent works.
            if object["type"] as? String == "queue-operation" {
                let content = (object["content"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
                switch object["operation"] as? String {
                case "enqueue":
                    // Notifications and other injected wrappers are queued the same way; they are not the user's words.
                    if !content.isEmpty, !content.hasPrefix("<") {
                        waiting.append(Waiting(content: content, timestamp: timestamp, offset: source.offset))
                    }
                case "dequeue":
                    // Taken while idle: the user line that follows is the message, so it must still be shown.
                    if !waiting.isEmpty { waiting.removeFirst() }
                case "remove":
                    guard !content.isEmpty, !content.hasPrefix("<") else { break }
                    // Absorbed into the running turn: from here it counts as sent. (The attachment that carries it
                    // follows and is skipped as a duplicate.)
                    if let index = waiting.firstIndex(where: { $0.content == content }) { waiting.remove(at: index) }
                    if !wasTaken(content, near: timestamp) {
                        addUser(collapsePastes(content), timestamp: timestamp, seed: (object["commandUuid"] as? String) ?? "\(path):q\(index)", offset: source.offset, merge: false)
                        taken.append((content, timestamp))
                    }
                default:
                    break
                }
                continue
            }

            // The queued message delivered into the turn.
            if object["type"] as? String == "attachment", let attachment = object["attachment"] as? [String: Any],
               attachment["type"] as? String == "queued_command" {
                let origin = (attachment["origin"] as? [String: Any])?["kind"] as? String
                var text = ""
                if let plain = attachment["prompt"] as? String {
                    text = plain
                } else if let blocks = attachment["prompt"] as? [[String: Any]] {
                    text = blocks.compactMap { $0["type"] as? String == "text" ? $0["text"] as? String : nil }.joined(separator: "\n\n")
                }
                text = collapsePastes(text).trimmingCharacters(in: .whitespacesAndNewlines)
                let pictures = ChatImages.refs(in: attachment["prompt"], lineOffset: source.offset, stripped: stripped).map(\.ref)
                guard origin == nil || origin == "human", !text.isEmpty || !pictures.isEmpty, !text.hasPrefix("<") else { continue }
                if let index = waiting.firstIndex(where: { $0.content == text }) { waiting.remove(at: index) }
                if wasTaken(text, near: timestamp) { continue }
                addUser(text, timestamp: timestamp, seed: (object["uuid"] as? String) ?? "\(path):a\(index)", offset: source.offset, merge: false, images: pictures)
                taken.append((text, timestamp))
                continue
            }

            // Claude Code's notice that it compacted the conversation.
            if object["type"] as? String == "system", object["subtype"] as? String == "compact_boundary" {
                flushTools()
                let before = (object["compactMetadata"] as? [String: Any])?["preTokens"] as? Int
                let note = before.map { "\($0 >= 1000 ? "\($0 / 1000)k" : "\($0)") tokens" } ?? ""
                drafts.append(Draft(role: .assistant, text: note, timestamp: timestamp, seed: (object["uuid"] as? String) ?? "\(path):c\(index)",
                                    toolSummary: nil, offset: source.offset, kind: .compaction))
                continue
            }
            // The summary that replaces the old conversation is not a message of the user's.
            if object["isCompactSummary"] as? Bool == true { continue }

            guard let type = object["type"] as? String, type == "user" || type == "assistant",
                  let message = object["message"] as? [String: Any] else { continue }
            if isMain, object["isSidechain"] as? Bool == true || object["isMeta"] as? Bool == true { continue }

            if type == "assistant", let model = message["model"] as? String, !model.hasPrefix("<") { latestModel = model }
            let seed = object["uuid"] as? String ?? "\(path):\(index)"

            var text = ""
            if let plain = message["content"] as? String {
                text = plain
            } else if let blocks = message["content"] as? [[String: Any]] {
                for block in blocks {
                    switch block["type"] as? String {
                    case "text":
                        if let part = block["text"] as? String { text += (text.isEmpty ? "" : "\n\n") + part }
                    case "tool_use":
                        if tally.isEmpty { tallyTimestamp = timestamp; tallySeed = seed; tallyOffset = source.offset }
                        let name = block["name"] as? String ?? ""
                        let input = block["input"] as? [String: Any] ?? [:]
                        tally.add(name, input)
                        let callId = block["id"] as? String ?? "\(seed):\(tallySteps.count)"
                        tallyIndex[callId] = tallySteps.count
                        tallySteps.append(Self.step(id: callId, name: name, input: input))
                    case "tool_result":
                        if let callId = block["tool_use_id"] as? String { attachResult(callId, Self.resultText(block["content"])) }
                    default:
                        break
                    }
                }
            }
            text = collapsePastes(text).trimmingCharacters(in: .whitespacesAndNewlines)
            // Pictures: pasted ones belong to the message, a tool's belong under its row, files named in the text too.
            let found = ChatImages.refs(in: message["content"], lineOffset: source.offset, stripped: stripped)
            var pictures = found.filter { !$0.inToolResult }.map(\.ref)
            let toolPictures = found.filter { $0.inToolResult }.map(\.ref)
            let marked = ChatImages.fileMarkers(in: text)
            text = marked.text
            pictures += marked.refs
            if !toolPictures.isEmpty { tallyImages += toolPictures }
            // A `!` command and what it printed, as Claude Code logs them.
            if type == "user" {
                if let command = Self.tagged(text, "bash-input") {
                    flushTools()
                    drafts.append(Draft(role: .user, text: "!" + command, timestamp: timestamp, seed: seed, toolSummary: nil, offset: source.offset, kind: .shell))
                    continue
                }
                if text.contains("<bash-stdout>") || text.contains("<bash-stderr>") {
                    let out = [Self.tagged(text, "bash-stdout"), Self.tagged(text, "bash-stderr")].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: "\n")
                    if let last = drafts.indices.last, drafts[last].kind == .shell { drafts[last].output = Self.capOutput(out) }
                    continue
                }
            }
            let audio = type == "assistant" ? ChatAudio.refs(in: text, cwd: latestCwd) : []
            // Injected wrappers (<command-name>, <system-reminder>) and interrupt markers are not chat.
            if (text.isEmpty && pictures.isEmpty) || text.hasPrefix("<") || text.hasPrefix("[Request interrupted") { continue }

            if type == "user" {
                if let index = waiting.firstIndex(where: { $0.content == text }) { waiting.remove(at: index) }
                if wasTaken(text, near: timestamp) { continue }
                addUser(text, timestamp: timestamp, seed: seed, offset: source.offset, images: pictures)
                continue
            }
            flushTools()
            if let last = drafts.last, last.role == .assistant, last.kind == .text {
                if !text.isEmpty { drafts[drafts.count - 1].text += "\n\n" + text }
                drafts[drafts.count - 1].images += pictures
                for ref in audio where !drafts[drafts.count - 1].audio.contains(ref) { drafts[drafts.count - 1].audio.append(ref) }
            } else {
                drafts.append(Draft(role: .assistant, text: text, timestamp: timestamp, seed: seed, toolSummary: nil, offset: source.offset, images: pictures, audio: audio))
            }
        }
        flushTools()
        // Still waiting for the agent to take them: shown now, marked "đang chờ".
        for item in waiting {
            drafts.append(Draft(role: .user, text: collapsePastes(item.content), timestamp: item.timestamp,
                                seed: "queued:\(item.offset):\(item.content.hashValue)", toolSummary: nil, offset: item.offset, queued: true))
        }

        let messages = drafts.suffix(limit).map { draft in
            ChatMessage(id: stableUUID(draft.seed), role: draft.role, text: cap(draft.text),
                        timestamp: draft.timestamp, toolSummary: draft.toolSummary, queued: draft.queued, sourceOffset: draft.offset, images: draft.images, audio: draft.audio,
                        kind: draft.kind, steps: draft.steps, output: draft.output)
        }
        return ChatParseResult(messages: messages, malformedLines: malformed, model: latestModel)
    }

    // MARK: Tool steps

    private static let maxSummary = 140
    private static let maxOutput = 3000
    private static let maxDiffRows = 80

    static func capOutput(_ text: String) -> String {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.count > maxOutput ? String(trimmed.prefix(maxOutput - 1)) + "…" : trimmed
    }

    /// The text of a tool result: a plain string, or the text blocks of a list.
    static func resultText(_ content: Any?) -> String {
        if let plain = content as? String { return capOutput(plain) }
        if let blocks = content as? [[String: Any]] {
            return capOutput(blocks.compactMap { $0["type"] as? String == "text" ? $0["text"] as? String : nil }.joined(separator: "\n"))
        }
        return ""
    }

    /// The text between `<tag>` and `</tag>`, if the text has them.
    static func tagged(_ text: String, _ tag: String) -> String? {
        guard let open = text.range(of: "<\(tag)>"), let close = text.range(of: "</\(tag)>", range: open.upperBound..<text.endIndex) else { return nil }
        return String(text[open.upperBound..<close.lowerBound]).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static func shortPath(_ path: String) -> String {
        let parts = path.split(separator: "/").suffix(2)
        return parts.joined(separator: "/")
    }

    private static func firstLine(_ text: String) -> String {
        let line = text.split(separator: "\n", omittingEmptySubsequences: true).first.map(String.init) ?? text
        return line.count > maxSummary ? String(line.prefix(maxSummary - 1)) + "…" : line
    }

    private static func rows(_ text: String, _ kind: DiffRow.Kind, limit: Int) -> [DiffRow] {
        text.split(separator: "\n", omittingEmptySubsequences: false).prefix(limit).map { DiffRow(kind, String($0)) }
    }

    /// A call as the phone shows it: a name, one line of input, and for Edit/MultiEdit/Write the changed lines.
    static func step(id: String, name: String, input: [String: Any]) -> ToolStep {
        let file = (input["file_path"] as? String) ?? (input["notebook_path"] as? String) ?? (input["path"] as? String)
        switch name {
        case "Edit", "MultiEdit", "NotebookEdit":
            var diff: [DiffRow] = []
            var edits: [[String: Any]] = (input["edits"] as? [[String: Any]]) ?? []
            if edits.isEmpty { edits = [input] }
            for edit in edits {
                let old = (edit["old_string"] as? String) ?? ""
                let new = (edit["new_string"] as? String) ?? (edit["new_source"] as? String) ?? ""
                diff += rows(old, .removed, limit: 30) + rows(new, .added, limit: 30)
                if diff.count >= maxDiffRows { diff = Array(diff.prefix(maxDiffRows)); break }
            }
            return ToolStep(id: id, name: name, summary: file.map(shortPath) ?? name, path: file, diff: diff)
        case "Write":
            let content = (input["content"] as? String) ?? ""
            return ToolStep(id: id, name: name, summary: file.map(shortPath) ?? name, path: file, diff: rows(content, .added, limit: 40))
        case "Bash":
            return ToolStep(id: id, name: name, summary: firstLine((input["command"] as? String) ?? ""))
        case "Read":
            return ToolStep(id: id, name: name, summary: file.map(shortPath) ?? name, path: file)
        case "Grep":
            let pattern = (input["pattern"] as? String) ?? ""
            return ToolStep(id: id, name: name, summary: firstLine(pattern + ((input["path"] as? String).map { " in \(shortPath($0))" } ?? "")))
        case "Glob":
            return ToolStep(id: id, name: name, summary: firstLine((input["pattern"] as? String) ?? ""))
        default:
            let text = input.values.compactMap { $0 as? String }.first ?? ""
            return ToolStep(id: id, name: name, summary: text.isEmpty ? name : firstLine(text))
        }
    }

    private static func cap(_ text: String) -> String {
        text.count > maxTextCharacters ? String(text.prefix(maxTextCharacters - 1)) + "…" : text
    }

    private static func parseISO(_ string: String) -> Date? {
        let fractional = ISO8601DateFormatter()
        fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return fractional.date(from: string) ?? ISO8601DateFormatter().date(from: string)
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
}

/// Counts tool calls the way the terminal collapses them: "Read 2 files, searched for 1 pattern".
struct ToolTally {
    private var order: [String] = []
    private var counts: [String: Int] = [:]
    private var files: [String: Set<String>] = [:]

    var isEmpty: Bool { order.isEmpty }

    mutating func add(_ tool: String, _ input: [String: Any]) {
        let key: String
        switch tool {
        case "Read": key = "read"
        case "Edit", "MultiEdit", "NotebookEdit": key = "edit"
        case "Write": key = "write"
        case "Grep", "Glob": key = "search"
        case "Bash": key = Self.bashKind(input["command"] as? String ?? "")
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
        func count(_ key: String, _ one: String, _ many: String) -> String {
            let total = files[key]?.count ?? counts[key] ?? 0
            return "\(total) \(total == 1 ? one : many)"
        }
        let parts = order.map { key -> String in
            switch key {
            case "read": return "read " + count(key, "file", "files")
            case "edit": return "edited " + count(key, "file", "files")
            case "write": return "wrote " + count(key, "file", "files")
            case "search": return "searched for " + count(key, "pattern", "patterns")
            case "bash": return "ran " + count(key, "shell command", "shell commands")
            case "web": return "searched the web"
            case "agent": return "ran " + count(key, "agent", "agents")
            case "ask": return "asked you"
            case "tasks": return "updated tasks"
            default: return "used " + key.dropFirst(4)
            }
        }
        let text = parts.joined(separator: ", ")
        return text.prefix(1).uppercased() + text.dropFirst()
    }

    /// The terminal counts a shell command that only searches or reads as a search or a read.
    private static func bashKind(_ command: String) -> String {
        let words = command.components(separatedBy: CharacterSet(charactersIn: "&|;\n"))
            .compactMap { $0.split(separator: " ").first.map(String.init) }
            .filter { $0 != "cd" }
        if !words.isEmpty, words.allSatisfy({ ["grep", "rg", "find", "egrep"].contains($0) }) { return "search" }
        if !words.isEmpty, words.allSatisfy({ ["cat", "head", "tail", "sed", "less", "wc"].contains($0) }) { return "read" }
        return "bash"
    }
}

/// Reads transcripts for the chat, and reads them cheaply:
/// - the newest part (the last 2 MB) is kept per file and, when the file only grew, only the bytes from the last
///   message on are parsed again;
/// - older history is read page by page, back to the first line, by byte offset, so a long transcript (tens of
///   megabytes) is never read whole. Every message knows where it starts in the file.
public final class ChatTranscriptCache {
    private struct Entry {
        var modified: Date
        var size: UInt64
        var isMain: Bool
        /// Where the cached window starts (0 when it reaches the first line).
        var windowStart: UInt64
        var messages: [ChatMessage]
        var model: String?
        var malformed: Int
    }

    private let lock = NSLock()
    private var entries: [URL: Entry] = [:]
    /// message id -> offset of its first line, for paging backwards from it.
    private var offsets: [URL: [UUID: UInt64]] = [:]
    /// Pictures the chat of each transcript has shown. Only these can be fetched: a phone cannot name another file.
    private var pictures: [URL: [String: ImageRef]] = [:]
    /// Audio files the chat of each transcript has named; the same rule as pictures.
    private var sounds: [URL: [String: AudioRef]] = [:]

    public init() {}

    public func messages(at url: URL, isMain: Bool, limit: Int = ChatTranscript.defaultLimit) -> [ChatMessage] {
        result(at: url, isMain: isMain).messages.suffix(limit).map { $0 }
    }

    public func result(at url: URL, isMain: Bool) -> ChatParseResult {
        guard let entry = refresh(url: url, isMain: isMain) else { return .empty }
        return ChatParseResult(messages: Array(entry.messages.suffix(ChatTranscript.defaultLimit)), malformedLines: entry.malformed, model: entry.model)
    }

    /// The newest messages, up to the 200 kept, with whether the file has more before them.
    private func refresh(url: URL, isMain: Bool) -> Entry? {
        let attributes: [FileAttributeKey: Any]
        do {
            attributes = try FileManager.default.attributesOfItem(atPath: url.path)
        } catch {
            ConsoleLog.error("messages", url.path, "stat failed: \(error)")
            return nil
        }
        let modified = attributes[.modificationDate] as? Date ?? .distantPast
        let size = (attributes[.size] as? NSNumber)?.uint64Value ?? 0

        lock.lock()
        let cached = entries[url]
        lock.unlock()
        if let cached, cached.modified == modified, cached.size == size, cached.isMain == isMain { return cached }

        var entry: Entry
        if var previous = cached, previous.isMain == isMain, size > previous.size, !previous.messages.isEmpty {
            // The file only grew: parse again from the oldest message that can still change (the last one, or the
            // first message still waiting in the queue), keep everything before it.
            let resumeIndex = previous.messages.firstIndex(where: { $0.queued }) ?? previous.messages.count - 1
            let resumeFrom = previous.messages[resumeIndex...].compactMap(\.sourceOffset).min() ?? previous.messages[resumeIndex].sourceOffset ?? 0
            if let window = ChatTranscript.readWindow(url: url, from: resumeFrom, to: nil) {
                let parsed = ChatTranscript.parse(sourceLines: window.lines, isMain: isMain, path: url.path, limit: .max)
                previous.messages = Array(previous.messages[..<resumeIndex]) + parsed.messages
                previous.messages = Array(previous.messages.suffix(ChatTranscript.defaultLimit * 2))
                previous.model = parsed.model ?? previous.model
                previous.modified = modified
                previous.size = size
                entry = previous
            } else {
                return cached
            }
        } else {
            guard let window = ChatTranscript.readWindow(url: url, from: nil, to: nil) else { return nil }
            let parsed = ChatTranscript.parse(sourceLines: window.lines, isMain: isMain, path: url.path, limit: .max)
            entry = Entry(modified: modified, size: size, isMain: isMain, windowStart: window.start, messages: parsed.messages,
                          model: parsed.model, malformed: parsed.malformedLines)
        }
        lock.lock()
        entries[url] = entry
        var known = offsets[url] ?? [:]
        for message in entry.messages { if let offset = message.sourceOffset { known[message.id] = offset } }
        offsets[url] = known
        remember(entry.messages, of: url)
        lock.unlock()
        return entry
    }

    // MARK: - Pictures

    /// Call with the lock held.
    private func remember(_ messages: [ChatMessage], of url: URL) {
        var known = pictures[url] ?? [:]
        for message in messages { for ref in message.images { known[ref.id] = ref } }
        pictures[url] = known
        var heard = sounds[url] ?? [:]
        for message in messages { for ref in message.audio { heard[ref.id] = ref } }
        sounds[url] = heard
    }

    /// A piece of an audio file this transcript's chat has named.
    public func audio(at url: URL, id: String, offset: Int, length: Int,
                      home: URL = FileManager.default.homeDirectoryForCurrentUser) throws -> ChatAudio.Chunk {
        lock.lock()
        let ref = sounds[url]?[id]
        lock.unlock()
        guard let ref else { throw AudioError.notFound }
        return try ChatAudio.chunk(of: ref, offset: offset, length: length, home: home)
    }

    /// The JPEG of a picture this transcript's chat has shown, at most `maxWidth` wide.
    public func image(at url: URL, id: String, maxWidth: Int, home: URL = FileManager.default.homeDirectoryForCurrentUser) throws -> Data {
        lock.lock()
        let ref = pictures[url]?[id]
        lock.unlock()
        guard let ref else { throw ImageError.notFound }
        return try ChatImages.jpeg(from: try ChatImages.data(for: ref, transcript: url, home: home), maxWidth: maxWidth)
    }

    // MARK: - Paging

    public struct Page: Sendable {
        public var messages: [ChatMessage]
        public var hasMore: Bool
        public var model: String?
    }

    /// The newest `limit` messages. `hasMore` is true while the file has anything before the first one returned.
    public func latest(at url: URL, isMain: Bool, limit: Int) -> Page? {
        guard let entry = refresh(url: url, isMain: isMain) else { return nil }
        let page = Array(entry.messages.suffix(limit))
        let hasMore = entry.messages.count > limit || entry.windowStart > 0
        return Page(messages: page, hasMore: hasMore, model: entry.model)
    }

    /// The `limit` messages just before `before`, reading back through the file as far as needed. Nil when `before`
    /// is not a message of this file that was served earlier.
    public func page(at url: URL, isMain: Bool, before: UUID, limit: Int) -> Page? {
        _ = refresh(url: url, isMain: isMain)
        lock.lock()
        let end = offsets[url]?[before]
        lock.unlock()
        guard let end else { return nil }
        if end == 0 { return Page(messages: [], hasMore: false, model: nil) }

        var span: UInt64 = 512 * 1024
        while true {
            let from = end > span ? end - span : 0
            guard let window = ChatTranscript.readWindow(url: url, from: from, to: end) else { return nil }
            var messages = ChatTranscript.parse(sourceLines: window.lines, isMain: isMain, path: url.path, limit: .max).messages
            // The first message of a window that began mid-file may be cut short (its earlier lines are outside).
            if window.start > 0, !messages.isEmpty { messages.removeFirst() }
            if messages.count >= limit || from == 0 || span >= 64 * 1024 * 1024 {
                let chosen = Array(messages.suffix(limit))
                lock.lock()
                var known = offsets[url] ?? [:]
                for message in chosen { if let offset = message.sourceOffset { known[message.id] = offset } }
                offsets[url] = known
                remember(chosen, of: url)
                lock.unlock()
                let hasMore = messages.count > limit || (from > 0 && (chosen.first?.sourceOffset ?? 0) > 0)
                return Page(messages: chosen, hasMore: hasMore, model: nil)
            }
            span *= 4
        }
    }

    /// Messages newer than `after`, from the newest part of the file. `found` is false when `after` is no longer in it.
    public func newer(at url: URL, isMain: Bool, after: UUID, limit: Int) -> (messages: [ChatMessage], found: Bool, hasMore: Bool)? {
        guard let entry = refresh(url: url, isMain: isMain) else { return nil }
        guard let index = entry.messages.firstIndex(where: { $0.id == after }) else { return ([], false, false) }
        let newer = Array(entry.messages[(index + 1)...])
        return (Array(newer.suffix(limit)), true, newer.count > limit)
    }
}
