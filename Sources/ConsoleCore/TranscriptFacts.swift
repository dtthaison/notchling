import Foundation

public struct MermaidBlock: Equatable, Sendable {
    public var source: String
    public var origin: String
    public var time: Date?
    public var messageId: String
}

/// Everything the Visualize tabs need from one transcript, read in a single pass.
public struct TranscriptFacts: Equatable, Sendable {
    public var turns: [FlowTurn] = []
    public var fileEdits: [String: Int] = [:]
    public var mermaid: [MermaidBlock] = []
    public var usage: [UsagePoint] = []
    public var toolCounts: [String: Int] = [:]
    /// http(s)://localhost or 127.0.0.1 URLs in order of appearance.
    public var localURLs: [String] = []
    /// Device names seen next to simctl or adb, e.g. "iPhone 16 Pro".
    public var deviceHints: [String] = []
    public var newestActivity: Date?

    public init() {}

    public mutating func merge(_ other: TranscriptFacts) {
        for (path, count) in other.fileEdits { fileEdits[path, default: 0] += count }
        mermaid += other.mermaid
        usage += other.usage
        for (tool, count) in other.toolCounts { toolCounts[tool, default: 0] += count }
        localURLs += other.localURLs
        deviceHints += other.deviceHints.filter { !deviceHints.contains($0) }
        if let date = other.newestActivity { newestActivity = max(newestActivity ?? date, date) }
    }
}

public enum TranscriptAnalyzer {
    public static let tailBytes: UInt64 = 8_000_000
    private static let editTools: Set<String> = ["Edit", "Write", "MultiEdit", "NotebookEdit"]
    private static let urlPattern = try! NSRegularExpression(
        pattern: #"https?://(?:localhost|127\.0\.0\.1):\d{2,5}[^\s"'<>)\]\\`,]*"#)
    private static let devicePattern = try! NSRegularExpression(
        pattern: #"(?:iPhone|iPad)(?: [0-9]{1,2}[a-z]?)?(?: (?:Pro Max|Pro|Plus|mini|Air|e))?"#)
    private static let mermaidPattern = try! NSRegularExpression(
        pattern: #"```mermaid[ \t]*\n(.*?)```"#, options: [.dotMatchesLineSeparators])

    /// `isMain` keeps only the main thread's own lines for the timeline and usage; a subagent file
    /// is all sidechain, so it keeps everything.
    public static func analyze(url: URL, isMain: Bool, origin: String, tailBytes: UInt64 = tailBytes) -> TranscriptFacts {
        guard let lines = ChatTranscript.tail(of: url, bytes: tailBytes) else { return TranscriptFacts() }
        return analyze(lines: lines, isMain: isMain, origin: origin, path: url.path)
    }

    public static func analyze(lines: [Substring], isMain: Bool, origin: String, path: String = "") -> TranscriptFacts {
        var facts = TranscriptFacts()
        var turns: [FlowTurn] = []
        var usageById: [String: UsagePoint] = [:]
        var usageOrder: [String] = []

        for (index, line) in lines.enumerated() {
            if line.utf8.count > ChatTranscript.maxLineBytes { continue }
            guard let object = parse(line, index: index, path: path),
                  let type = object["type"] as? String, type == "user" || type == "assistant",
                  let message = object["message"] as? [String: Any] else { continue }
            if isMain, object["isSidechain"] as? Bool == true || object["isMeta"] as? Bool == true { continue }

            let time = (object["timestamp"] as? String).flatMap(parseISO)
            if let time { facts.newestActivity = max(facts.newestActivity ?? time, time) }
            let blocks = message["content"] as? [[String: Any]] ?? []

            if type == "user" {
                let prompt: String
                if let plain = message["content"] as? String {
                    prompt = plain
                } else {
                    prompt = blocks.filter { $0["type"] as? String == "text" }.compactMap { $0["text"] as? String }.joined(separator: "\n")
                }
                collectURLs(in: prompt, into: &facts)
                for block in blocks where block["type"] as? String == "tool_result" {
                    // A dev server's banner ("Local: http://localhost:5173") arrives as a tool result.
                    if let output = block["content"] as? String { collectURLs(in: output, into: &facts) }
                    else if let parts = block["content"] as? [[String: Any]] {
                        for part in parts { if let output = part["text"] as? String { collectURLs(in: output, into: &facts) } }
                    }
                }
                let cleaned = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
                if isMain, !cleaned.isEmpty, !cleaned.hasPrefix("<"), !cleaned.hasPrefix("[Request interrupted") {
                    turns.append(FlowTurn(index: turns.count + 1, prompt: String(cleaned.prefix(140)), calls: []))
                }
                continue
            }

            // Assistant: usage, tool calls, mermaid blocks.
            let messageId = message["id"] as? String ?? (object["uuid"] as? String ?? "\(path):\(index)")
            if let usage = message["usage"] as? [String: Any] {
                // Claude Code writes the same message several times while streaming; the last wins.
                if usageById[messageId] == nil { usageOrder.append(messageId) }
                usageById[messageId] = UsagePoint(
                    time: (time ?? .distantPast).timeIntervalSince1970 * 1000,
                    input: (usage["input_tokens"] as? Int) ?? 0,
                    output: (usage["output_tokens"] as? Int) ?? 0,
                    cacheCreation: (usage["cache_creation_input_tokens"] as? Int) ?? 0,
                    cacheRead: (usage["cache_read_input_tokens"] as? Int) ?? 0)
            }
            for block in blocks {
                switch block["type"] as? String {
                case "tool_use":
                    let name = block["name"] as? String ?? ""
                    let input = block["input"] as? [String: Any] ?? [:]
                    facts.toolCounts[name, default: 0] += 1
                    let call = flowCall(name: name, input: input, time: time)
                    if editTools.contains(name), let file = input["file_path"] as? String ?? input["notebook_path"] as? String {
                        facts.fileEdits[file, default: 0] += 1
                    }
                    if isMain {
                        if turns.isEmpty { turns.append(FlowTurn(index: 1, prompt: "", calls: [])) }
                        turns[turns.count - 1].calls.append(call)
                    }
                    if name == "Bash", let command = input["command"] as? String,
                       command.contains("simctl") || command.contains("adb") || command.contains("emulator") {
                        collectDevices(in: command, into: &facts)
                    }
                case "text":
                    guard let body = block["text"] as? String else { continue }
                    collectURLs(in: body, into: &facts)
                    for source in mermaidSources(in: body) {
                        facts.mermaid.append(MermaidBlock(source: source, origin: origin, time: time, messageId: messageId))
                    }
                default:
                    break
                }
            }
        }
        facts.turns = turns
        facts.usage = usageOrder.compactMap { usageById[$0] }
        return facts
    }

    public static func mermaidSources(in text: String) -> [String] {
        guard text.contains("```mermaid") else { return [] }
        let range = NSRange(text.startIndex..., in: text)
        return mermaidPattern.matches(in: text, range: range).compactMap { match in
            Range(match.range(at: 1), in: text).map { String(text[$0]).trimmingCharacters(in: .whitespacesAndNewlines) }
        }.filter { !$0.isEmpty }
    }

    static func flowCall(name: String, input: [String: Any], time: Date?) -> FlowCall {
        func base(_ key: String = "file_path") -> String { ((input[key] as? String ?? "") as NSString).lastPathComponent }
        let target: String
        var subagent = false
        switch name {
        case "Read", "Edit", "Write", "MultiEdit": target = base()
        case "NotebookEdit": target = base("notebook_path")
        case "Bash": target = String((input["command"] as? String ?? "").replacingOccurrences(of: "\n", with: " ").prefix(60))
        case "Agent", "Task":
            subagent = true
            target = String((input["description"] as? String ?? input["subagent_type"] as? String ?? "subagent").prefix(60))
        case "Grep", "Glob": target = String((input["pattern"] as? String ?? "").prefix(60))
        case "WebFetch": target = String((input["url"] as? String ?? "").prefix(60))
        case "WebSearch": target = String((input["query"] as? String ?? "").prefix(60))
        default: target = ""
        }
        return FlowCall(tool: name, target: target, subagent: subagent,
                        edit: ["Edit", "Write", "MultiEdit", "NotebookEdit"].contains(name),
                        time: time.map { $0.timeIntervalSince1970 * 1000 })
    }

    private static func collectURLs(in text: String, into facts: inout TranscriptFacts) {
        guard text.contains("://") else { return }
        let range = NSRange(text.startIndex..., in: text)
        for match in urlPattern.matches(in: text, range: range) {
            guard let swiftRange = Range(match.range, in: text) else { continue }
            var url = String(text[swiftRange])
            while let last = url.last, ".,;:".contains(last) { url.removeLast() }
            facts.localURLs.append(url)
        }
    }

    private static func collectDevices(in text: String, into facts: inout TranscriptFacts) {
        let range = NSRange(text.startIndex..., in: text)
        for match in devicePattern.matches(in: text, range: range) {
            guard let swiftRange = Range(match.range, in: text) else { continue }
            let name = String(text[swiftRange])
            if !facts.deviceHints.contains(name) { facts.deviceHints.append(name) }
        }
    }

    private static func parse(_ line: Substring, index: Int, path: String) -> [String: Any]? {
        do {
            return try JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any]
        } catch {
            ConsoleLog.error("TranscriptAnalyzer.parse", path, "line \(index) is malformed, skipped: \(error)")
            return nil
        }
    }

    private static func parseISO(_ string: String) -> Date? {
        let fractional = ISO8601DateFormatter()
        fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return fractional.date(from: string) ?? ISO8601DateFormatter().date(from: string)
    }
}
