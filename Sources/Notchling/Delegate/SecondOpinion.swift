import Foundation

/// A second opinion asked from Chat: the prompt built from a change or a message, the arguments for
/// `Notchling --delegate`, and the reading of its `--json` answer. These are the parts that need no
/// process or view, so they can be tested; the store that runs and keeps them is `SecondOpinionStore`.
enum SecondOpinionPrompt {
    enum Subject: Equatable {
        /// An Edit or Write step: the file and its diff.
        case change(path: String, diff: [DiffLine])
        /// An assistant message.
        case message(String)
    }

    /// What goes into the prompt, so a pasted log or a huge diff can't make it unreasonably long.
    static let bodyLimit = 24_000

    static func build(_ subject: Subject, folder: String) -> String {
        switch subject {
        case .change(let path, let diff):
            let body = clip(unified(diff))
            var lines = ["Review this change for bugs, regressions and missed edge cases. Be specific and concise; if it looks right, say so.", ""]
            lines.append("Folder: \(folder)")
            if !path.isEmpty { lines.append("File: \(path)") }
            lines += ["", "```diff", body, "```"]
            return lines.joined(separator: "\n")
        case .message(let text):
            let body = clip(text.trimmingCharacters(in: .whitespacesAndNewlines))
            return ["Review this answer from another AI agent for mistakes, gaps and claims that aren't supported. Be specific and concise; check against the files if it refers to them.",
                    "", "Folder: \(folder)", "", "Message:", "", body].joined(separator: "\n")
        }
    }

    /// The diff as unified text: `+`, `-` or a space in front of each line, hunk headers as they are.
    static func unified(_ lines: [DiffLine]) -> String {
        lines.map { line -> String in
            switch line.kind {
            case .add: return "+" + line.text
            case .remove: return "-" + line.text
            case .context: return " " + line.text
            case .hunk: return line.text
            }
        }.joined(separator: "\n")
    }

    static func clip(_ text: String) -> String {
        guard text.count > bodyLimit else { return text }
        return String(text.prefix(bodyLimit)) + "\n… (\(text.count - bodyLimit) more characters left out)"
    }

    /// A short name for what was reviewed, shown on the card.
    static func title(_ subject: Subject) -> String {
        switch subject {
        case .change(let path, _):
            let name = (path as NSString).lastPathComponent
            return name.isEmpty ? "Change" : name
        case .message: return "Reply"
        }
    }
}

/// What `Notchling --delegate … --json` prints, read back.
struct SecondOpinionResult: Equatable {
    var ok: Bool
    var reply: String
    var session: String?
    var model: String?
    var effort: String?
    var error: String?
    var seconds: Double
    var tokens: Int?
    var timedOut: Bool

    /// The last JSON object in `output` (the command prints one line; anything before it is noise).
    static func parse(_ output: String) -> SecondOpinionResult? {
        for line in output.split(whereSeparator: \.isNewline).reversed() {
            let text = line.trimmingCharacters(in: .whitespaces)
            guard text.hasPrefix("{"),
                  let d = (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any],
                  d["tool"] != nil else { continue }
            return SecondOpinionResult(
                ok: d["ok"] as? Bool ?? false, reply: d["reply"] as? String ?? "", session: d["session"] as? String,
                model: d["model"] as? String, effort: d["effort"] as? String, error: d["error"] as? String,
                seconds: (d["seconds"] as? NSNumber)?.doubleValue ?? 0,
                tokens: d["tokens"] as? Int, timedOut: d["timedOut"] as? Bool ?? false)
        }
        return nil
    }
}

/// One second opinion: asked, running or finished. Kept per Chat session and across launches.
struct SecondOpinion: Identifiable, Codable, Equatable {
    enum Status: String, Codable { case running, done, failed, cancelled }

    var id = UUID().uuidString
    /// The Chat row id the opinion was asked in.
    var session: String
    /// The message item or tool call the card sits under.
    var anchor: String
    /// What was reviewed ("Reply", a file name).
    var subject: String
    /// `Delegate.Tool.rawValue`.
    var tool: String
    var model: String?
    var effort: String?
    var folder: String
    var started = Date()
    var status = Status.running
    var reply = ""
    var error: String?
    /// The tool's own session, which Chat lists with the others.
    var delegatedSession: String?
    var seconds: Double = 0
    var tokens: Int?

    var toolValue: Delegate.Tool { Delegate.Tool(rawValue: tool) ?? .codex }

    /// The arguments for `Notchling --delegate`: read-only (no `--write`), the prompt from a file so a long
    /// diff isn't an argument.
    static func arguments(tool: Delegate.Tool, model: String?, effort: String?, folder: String,
                          promptFile: String, timeout: TimeInterval = 900) -> [String] {
        var args = ["--delegate", tool.rawValue]
        if let model, !model.isEmpty { args += ["--model", model] }
        if let effort, !effort.isEmpty { args += ["--effort", effort] }
        args += ["--folder", folder, "--timeout", String(Int(timeout)), "--json", "--prompt-file", promptFile]
        return args
    }

    /// Applies a finished run's output.
    mutating func finish(with result: SecondOpinionResult?, output: String, cancelled: Bool) {
        if cancelled {
            status = .cancelled
            if let result, !result.reply.isEmpty { reply = result.reply }
            delegatedSession = result?.session ?? delegatedSession
            return
        }
        guard let result else {
            status = .failed
            let tail = output.trimmingCharacters(in: .whitespacesAndNewlines)
            error = tail.isEmpty ? "The tool ended without an answer." : String(tail.suffix(300))
            return
        }
        reply = result.reply
        model = result.model ?? model
        effort = result.effort ?? effort
        delegatedSession = result.session
        seconds = result.seconds
        tokens = result.tokens
        error = result.error
        status = result.ok && !result.reply.isEmpty ? .done : .failed
        if result.ok && result.reply.isEmpty { error = "It sent no reply." }
    }
}

/// The models a tool offers for the popover: Codex's cached list, Antigravity's `agy models`.
enum SecondOpinionModels {
    struct Option: Equatable, Hashable {
        let model: String
        let name: String
        let efforts: [String]
    }

    static func codex() -> [Option] {
        CodexModels.load().map { Option(model: $0.slug, name: $0.name, efforts: $0.efforts) }
    }

    /// Runs `agy models`; call off the main thread.
    static func agy() -> [Option] {
        guard let binary = AntigravityRunner.binary() else { return [] }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: binary)
        p.arguments = ["models"]
        let out = Pipe()
        p.standardOutput = out
        p.standardError = FileHandle.nullDevice
        p.standardInput = FileHandle.nullDevice
        do { try p.run() } catch { return [] }
        let data = out.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        guard p.terminationStatus == 0 else { return [] }
        return options(agyListing: String(decoding: data, as: UTF8.self))
    }

    static func options(agyListing: String) -> [Option] {
        Delegate.agyModels(agyListing).map { Option(model: $0.model, name: $0.model, efforts: $0.levels) }
    }
}
