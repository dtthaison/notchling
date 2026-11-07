import Foundation

/// A subagent a session started with the Agent tool. Claude Code 2.1.31x keeps each one's transcript
/// beside the session's: `<session>.jsonl` → `<session>/subagents/agent-<id>.jsonl`, with
/// `agent-<id>.meta.json` holding its type, description, model, effort, the Agent call's tool use id
/// and `requestShape` ("background" when it runs on while the session goes on).
struct Subagent: Identifiable, Equatable {
    enum Status: Equatable {
        case running, completed, failed, stopped
        /// No word that it finished, but nothing written for a while: the session may have stopped.
        case quiet
    }

    let id: String
    let path: String
    var description: String
    var type: String
    var model: String?
    var effort: String?
    var background: Bool
    var toolUseID: String?
    var started: Date
    var updated: Date
    var status: Status = .quiet

    var isRunning: Bool { status == .running }
}

enum Subagents {
    /// A subagent that wrote nothing for this long, with no word that it finished, counts as quiet.
    static let quietAfter: TimeInterval = 600
    /// The notch's quicker guess, without reading the session's transcript.
    static let activeWindow: TimeInterval = 90

    /// `<session>/subagents` for a session transcript.
    static func folder(forTranscript path: String) -> String {
        ((path as NSString).deletingPathExtension as NSString).appendingPathComponent("subagents")
    }

    /// Every subagent of a session, oldest first. `finished` maps an agent id (background) or an Agent
    /// call's tool use id (foreground) to how it ended, from the session's transcript.
    static func list(forTranscript path: String, finished: [String: String] = [:], now: Date = Date()) -> [Subagent] {
        let dir = folder(forTranscript: path)
        guard let names = try? FileManager.default.contentsOfDirectory(atPath: dir) else { return [] }
        var out: [Subagent] = []
        for name in names where name.hasPrefix("agent-") && name.hasSuffix(".jsonl") {
            let id = String(name.dropFirst(6).dropLast(6))
            let file = (dir as NSString).appendingPathComponent(name)
            let attrs = try? FileManager.default.attributesOfItem(atPath: file)
            let updated = attrs?[.modificationDate] as? Date ?? .distantPast
            let started = attrs?[.creationDate] as? Date ?? updated
            let meta = readMeta((dir as NSString).appendingPathComponent("agent-\(id).meta.json"))
            var agent = Subagent(id: id, path: file,
                                 description: meta["description"] as? String ?? "Subagent",
                                 type: meta["agentType"] as? String ?? "general-purpose",
                                 model: meta["model"] as? String, effort: meta["effort"] as? String,
                                 background: (meta["requestShape"] as? String) == "background",
                                 toolUseID: meta["toolUseId"] as? String,
                                 started: started, updated: updated)
            agent.status = status(of: agent, finished: finished, now: now)
            // The session's notice can be older than the part of its transcript that was read.
            if agent.status != .completed, agent.status != .failed, handedBack(file) { agent.status = .completed }
            out.append(agent)
        }
        return out.sorted { $0.started < $1.started }
    }

    static func status(of agent: Subagent, finished: [String: String], now: Date) -> Subagent.Status {
        // A background agent's tool call returns at once ("launched"); only its notification says it ended.
        let word = finished[agent.id] ?? (agent.background ? nil : agent.toolUseID.flatMap { finished[$0] })
        switch word {
        case "completed": return .completed
        case "failed", "error": return .failed
        case "killed", "stopped", "cancelled": return .stopped
        default: return now.timeIntervalSince(agent.updated) < quietAfter ? .running : .quiet
        }
    }

    /// A background subagent's last step is handing its report back: the tool result says
    /// "Report delivered to your caller" (2.1.318). Read from the last 8 KB.
    static func handedBack(_ path: String) -> Bool {
        guard let handle = FileHandle(forReadingAtPath: path) else { return false }
        defer { try? handle.close() }
        let size = (try? handle.seekToEnd()) ?? 0
        try? handle.seek(toOffset: size > 8192 ? size - 8192 : 0)
        guard let tail = try? handle.readToEnd() else { return false }
        return tail.range(of: Data("Report delivered to your caller".utf8)) != nil
    }

    /// How many look busy right now: written to in the last `activeWindow`. For the notch's rows.
    static func activeCount(forTranscript path: String, now: Date = Date()) -> Int {
        let dir = folder(forTranscript: path)
        guard let names = try? FileManager.default.contentsOfDirectory(atPath: dir) else { return 0 }
        return names.filter { name in
            guard name.hasSuffix(".jsonl"),
                  let modified = (try? FileManager.default.attributesOfItem(atPath: (dir as NSString).appendingPathComponent(name)))?[.modificationDate] as? Date
            else { return false }
            return now.timeIntervalSince(modified) < activeWindow
        }.count
    }

    /// `<task-id>…</task-id>` and `<status>…</status>` pairs in a task notification.
    static func notificationStatuses(in text: String) -> [(id: String, status: String)] {
        guard text.contains("<task-notification>") else { return [] }
        var out: [(String, String)] = []
        var rest = Substring(text)
        while let open = rest.range(of: "<task-notification>") {
            rest = rest[open.upperBound...]
            let end = rest.range(of: "</task-notification>")?.lowerBound ?? rest.endIndex
            let block = rest[..<end]
            if let id = tag("task-id", in: block), let status = tag("status", in: block) { out.append((id, status)) }
            rest = rest[end...]
        }
        return out
    }

    private static func tag(_ name: String, in text: Substring) -> String? {
        guard let a = text.range(of: "<\(name)>"), let b = text.range(of: "</\(name)>", range: a.upperBound..<text.endIndex) else { return nil }
        return String(text[a.upperBound..<b.lowerBound]).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static let metaLock = NSLock()
    private static var metaCache: [String: [String: Any]] = [:]

    /// Written once when the subagent starts, so it's read once.
    private static func readMeta(_ path: String) -> [String: Any] {
        metaLock.lock(); defer { metaLock.unlock() }
        if let hit = metaCache[path] { return hit }
        guard let data = FileManager.default.contents(atPath: path),
              let obj = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else { return [:] }
        metaCache[path] = obj
        return obj
    }
}
