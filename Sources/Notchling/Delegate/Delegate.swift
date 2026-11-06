import Foundation

/// One job handed from an agent to another tool: `Notchling --delegate <codex|agy> [options] -- <prompt>`.
/// The command runs the other tool's turn itself and waits for its end (Codex's process exits, agy's
/// `session/prompt` answers), so a caller that runs it in the background is woken by its exit: nothing
/// polls. These are the parts that need no process, kept apart so they can be tested.
enum Delegate {
    enum Tool: String, CaseIterable {
        case codex, agy

        init?(name: String) {
            switch name.lowercased() {
            case "codex": self = .codex
            case "agy", "antigravity", "gemini": self = .agy
            default: return nil
            }
        }

        var title: String { self == .codex ? "Codex" : "Antigravity CLI" }
    }

    struct Options: Equatable {
        var tool: Tool
        var prompt: String
        var model: String?
        var effort: String?
        /// The folder the tool works in (its workspace).
        var folder: String
        /// Where generated images and videos are copied; nil leaves them where the tool saved them.
        var out: String?
        /// Lets the tool change files in `folder` (Codex workspace-write, agy accept-edits). Off, it only reads.
        var write = false
        /// Continue this session instead of starting one.
        var session: String?
        var timeout: TimeInterval = 1800
        var json = false
    }

    struct ParseError: Error, Equatable { let message: String }

    static let usage = """
    Usage: Notchling --delegate <codex|agy> [--model <m>] [--effort <e>] [--folder <dir>] [--out <dir>]
                      [--write] [--session <id>] [--timeout <seconds>] [--json] -- <prompt>
           Notchling --delegate-models [codex|agy]
    """

    /// Nested delegations stop here, so two agents can't keep handing a job back and forth.
    static let maxDepth = 2
    static let depthVariable = "NOTCHLING_DELEGATE_DEPTH"

    /// `args` are what follows `--delegate`.
    static func parse(_ args: [String], cwd: String) -> Result<Options, ParseError> {
        guard let first = args.first, let tool = Tool(name: first) else {
            return .failure(ParseError(message: "Name the tool first: codex or agy.\n\(usage)"))
        }
        var o = Options(tool: tool, prompt: "", folder: cwd)
        var i = 1
        var words: [String] = []
        func value(_ flag: String) -> Result<String, ParseError> {
            guard i + 1 < args.count, !args[i + 1].hasPrefix("--") else { return .failure(ParseError(message: "\(flag) needs a value.")) }
            i += 1
            return .success(args[i])
        }
        while i < args.count {
            let a = args[i]
            switch a {
            case "--":
                words += args[(i + 1)...]
                i = args.count
                continue
            case "--write": o.write = true
            case "--json": o.json = true
            case "--model", "--effort", "--folder", "--out", "--session", "--timeout", "--prompt-file":
                switch value(a) {
                case .failure(let e): return .failure(e)
                case .success(let v):
                    switch a {
                    case "--model": o.model = v
                    case "--effort": o.effort = v.lowercased()
                    case "--folder": o.folder = absolute(v, cwd: cwd)
                    case "--out": o.out = absolute(v, cwd: cwd)
                    case "--session": o.session = v
                    case "--timeout":
                        guard let t = TimeInterval(v), t > 0 else { return .failure(ParseError(message: "--timeout takes seconds.")) }
                        o.timeout = t
                    default:
                        guard let text = try? String(contentsOfFile: absolute(v, cwd: cwd), encoding: .utf8) else {
                            return .failure(ParseError(message: "Couldn't read \(v)."))
                        }
                        words.append(text)
                    }
                }
            default:
                if a.hasPrefix("--") { return .failure(ParseError(message: "Unknown option \(a).\n\(usage)")) }
                words.append(a)
            }
            i += 1
        }
        o.prompt = words.joined(separator: " ").trimmingCharacters(in: .whitespacesAndNewlines)
        guard !o.prompt.isEmpty else { return .failure(ParseError(message: "No prompt given.\n\(usage)")) }
        return .success(o)
    }

    static func absolute(_ path: String, cwd: String) -> String {
        let expanded = (path as NSString).expandingTildeInPath
        let url = expanded.hasPrefix("/") ? URL(fileURLWithPath: expanded) : URL(fileURLWithPath: cwd).appendingPathComponent(expanded)
        return url.standardizedFileURL.path
    }

    // MARK: Antigravity's model and effort

    /// agy's model ids (`agy models`) carry the thinking level as a suffix (`gemini-4-argon-high`), while a
    /// session sets the model (`gemini-4-argon`) and `thought_level` apart. Accepts either form; an effort
    /// given on its own wins over the suffix. Claude Code's levels map onto agy's three.
    static func agyPick(model: String?, effort: String?, models: [String]) -> (model: String?, effort: String?) {
        var base = model
        var level = effort.map(agyLevel)
        if let m = model, !models.contains(m) {
            for suffix in ["low", "medium", "high"] where m.hasSuffix("-" + suffix) {
                let stem = String(m.dropLast(suffix.count + 1))
                if models.contains(stem) {
                    base = stem
                    if level == nil { level = suffix }
                }
            }
            // A model offered only with its level in the name (`gpt-oss-120b-medium`).
            if let b = base, !models.contains(b), let whole = models.first(where: { $0.hasPrefix(b + "-") }) {
                base = whole
            }
        }
        return (base, level)
    }

    static func agyLevel(_ effort: String) -> String {
        switch effort.lowercased() {
        case "minimal", "none", "low": return "low"
        case "xhigh", "max", "high": return "high"
        default: return effort.lowercased()
        }
    }

    /// `agy models` lines (`id<TAB>name`) grouped as model → thinking levels, in the order listed.
    static func agyModels(_ listing: String) -> [(model: String, levels: [String])] {
        var order: [String] = []
        var levels: [String: [String]] = [:]
        for line in listing.split(whereSeparator: \.isNewline) {
            guard let id = line.split(separator: "\t").first.map(String.init), !id.contains(" ") else { continue }
            var model = id, level = ""
            for suffix in ["low", "medium", "high"] where id.hasSuffix("-" + suffix) {
                model = String(id.dropLast(suffix.count + 1))
                level = suffix
            }
            if levels[model] == nil { order.append(model); levels[model] = [] }
            if !level.isEmpty { levels[model]?.append(level) }
        }
        return order.map { ($0, levels[$0] ?? []) }
    }

    // MARK: Antigravity's subagents

    /// Where an agy turn stands once `session/prompt` has answered. agy's image generator (and other
    /// subagents) run on after the turn ends: each one leaves a message in the parent's mailbox when it's
    /// done, agy wakes the parent with it, and the parent's answer goes only to its transcript (no prompt
    /// is open, so ACP sends nothing). `system` is the session's `brain/<id>/.system_generated`.
    enum AgyProgress: Equatable {
        /// No subagent started since `since`: the turn's own reply stands.
        case finished
        /// Subagents still working, or the parent hasn't answered their messages yet.
        case waiting(subagents: [String])
        /// Every subagent reported and the parent answered after the last one.
        case answered(String, subagents: [String])
    }

    static func agyProgress(system: URL, since: Date) -> AgyProgress {
        let fm = FileManager.default
        func json(_ url: URL) -> [String: Any]? { (try? Data(contentsOf: url)).flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] } }
        func files(_ sub: String) -> [URL] {
            ((try? fm.contentsOfDirectory(at: system.appendingPathComponent(sub), includingPropertiesForKeys: [.contentModificationDateKey])) ?? [])
                .filter { $0.pathExtension == "json" }
        }
        func modified(_ url: URL) -> Date { (try? url.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate ?? .distantPast }

        let spawned = files("subagents").filter { modified($0) >= since.addingTimeInterval(-1) }
            .compactMap { json($0)?["conversationId"] as? String }
        guard !spawned.isEmpty else { return .finished }
        let reported = Set(files("messages").compactMap { json($0)?["sender"] as? String })
        guard spawned.allSatisfy(reported.contains) else { return .waiting(subagents: spawned) }

        // The parent's last answer must come after the system message that woke it.
        let lines = ((try? String(contentsOf: system.appendingPathComponent("logs/transcript.jsonl"), encoding: .utf8)) ?? "")
            .split(whereSeparator: \.isNewline)
            .compactMap { (try? JSONSerialization.jsonObject(with: Data($0.utf8))) as? [String: Any] }
        guard let wake = lines.lastIndex(where: { $0["type"] as? String == "SYSTEM_MESSAGE" }),
              let last = lines.last, lines.count - 1 > wake,
              last["type"] as? String == "PLANNER_RESPONSE", last["status"] as? String == "DONE",
              let text = (last["content"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines), !text.isEmpty
        else { return .waiting(subagents: spawned) }
        return .answered(text, subagents: spawned)
    }

    // MARK: Generated media

    static let mediaExtensions: Set<String> = ["png", "jpg", "jpeg", "webp", "gif", "heic", "mp4", "mov", "webm", "m4v", "mp3", "wav", "m4a"]

    /// Where each tool saves what its image (or video) tool made, per session.
    static func mediaFolder(_ tool: Tool, session: String) -> URL {
        let home = FileManager.default.homeDirectoryForCurrentUser
        switch tool {
        case .codex: return CodexStore.home.appendingPathComponent("generated_images/\(session)")
        case .agy: return home.appendingPathComponent(".gemini/antigravity-cli/brain/\(session)")
        }
    }

    /// Images, videos and audio directly in `folder` written at or after `since`, oldest first.
    static func media(in folder: URL, since: Date) -> [URL] {
        let keys: [URLResourceKey] = [.contentModificationDateKey, .isRegularFileKey]
        let files = (try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: keys)) ?? []
        return files.compactMap { url -> (URL, Date)? in
            guard mediaExtensions.contains(url.pathExtension.lowercased()),
                  let v = try? url.resourceValues(forKeys: Set(keys)), v.isRegularFile == true,
                  let date = v.contentModificationDate, date >= since.addingTimeInterval(-1) else { return nil }
            return (url, date)
        }
        .sorted { $0.1 < $1.1 }
        .map(\.0)
    }

    /// Copies `files` into `out`, never over an existing file (`name-2.png`, …). Returns the new paths.
    static func copy(_ files: [URL], to out: URL) throws -> [URL] {
        let fm = FileManager.default
        try fm.createDirectory(at: out, withIntermediateDirectories: true)
        return try files.map { file in
            let stem = file.deletingPathExtension().lastPathComponent, ext = file.pathExtension
            var target = out.appendingPathComponent(file.lastPathComponent)
            var n = 2
            while fm.fileExists(atPath: target.path) {
                target = out.appendingPathComponent("\(stem)-\(n)" + (ext.isEmpty ? "" : ".\(ext)"))
                n += 1
            }
            try fm.copyItem(at: file, to: target)
            return target
        }
    }

    // MARK: Result

    struct Outcome: Equatable {
        var tool: Tool
        var session: String?
        var model: String?
        var effort: String?
        var reply = ""
        var media: [String] = []
        var copied: [String] = []
        /// Permissions the tool asked for that were declined (agy), by title.
        var declined: [String] = []
        var error: String?
        var timedOut = false
        var seconds: Double = 0
        var tokens: Int?

        var exitCode: Int32 { timedOut ? 124 : (error == nil ? 0 : 1) }
    }

    static func render(_ o: Outcome) -> String {
        var lines: [String] = []
        var head = o.tool.title
        let pick = [o.model, o.effort].compactMap { $0 }.joined(separator: ", ")
        if !pick.isEmpty { head += " (\(pick))" }
        head += o.timedOut ? ": timed out" : (o.error == nil ? ": done" : ": failed")
        head += String(format: " in %.0fs", o.seconds)
        if let t = o.tokens { head += ", \(t) tokens" }
        lines.append(head)
        if let s = o.session { lines.append("Session: \(s)") }
        if let e = o.error { lines.append("Error: \(e)") }
        if !o.media.isEmpty { lines.append("Generated:"); lines += o.media.map { "  " + $0 } }
        if !o.copied.isEmpty { lines.append("Copied to:"); lines += o.copied.map { "  " + $0 } }
        if !o.declined.isEmpty { lines.append("Declined (not allowed in a delegated run):"); lines += o.declined.map { "  " + $0 } }
        if !o.reply.isEmpty { lines.append(""); lines.append(o.reply) }
        return lines.joined(separator: "\n")
    }

    static func json(_ o: Outcome) -> String {
        var d: [String: Any] = ["tool": o.tool.rawValue, "ok": o.error == nil, "seconds": Int(o.seconds.rounded()),
                                "reply": o.reply, "media": o.media, "copied": o.copied, "declined": o.declined]
        if let v = o.session { d["session"] = v }
        if let v = o.model { d["model"] = v }
        if let v = o.effort { d["effort"] = v }
        if let v = o.error { d["error"] = v }
        if let v = o.tokens { d["tokens"] = v }
        if o.timedOut { d["timedOut"] = true }
        let data = (try? JSONSerialization.data(withJSONObject: d, options: [.sortedKeys, .withoutEscapingSlashes])) ?? Data()
        return String(decoding: data, as: UTF8.self)
    }
}
