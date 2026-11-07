import Foundation

/// Decides, for each run that ended, who hears of it. It only decides: the watcher sends and notifies,
/// so this runs the same in a test.
///
/// The session that started a run gets one message, if it's a background conversation (the only kind
/// Notchling can type into). Anything else, such as a terminal session or `claude -p`, gets a notification.
struct RunWaker {
    enum Action: Equatable {
        /// `claude attach <job>` and type `text`.
        case send(job: String, text: String, id: String)
        case notify(title: String, body: String, id: String)

        var id: String {
            switch self {
            case .send(_, _, let id), .notify(_, _, let id): return id
            }
        }
    }

    /// A run waits this long for its agent to stop working before the user is told instead.
    static let queueLimit: TimeInterval = 6 * 3600

    /// Runs that ended before this are history: never announced.
    let startedAt: Date
    private(set) var handled = Set<String>()
    private var queued: [String: (end: RunEnd, since: Date)] = [:]

    init(startedAt: Date) { self.startedAt = startedAt }

    var queuedCount: Int { queued.count }

    /// `ends` are the recently finished runs; `agents` are all the Claude Code sessions, hidden ones too.
    mutating func plan(ends: [RunEnd], agents: [Agent], enabled: Bool, now: Date, lastLine: (String) -> String?) -> [Action] {
        var candidates = ends
        for (id, entry) in queued where !ends.contains(where: { $0.id == id }) { candidates.append(entry.end) }
        candidates.sort { $0.finish.at < $1.finish.at }

        var actions: [Action] = []
        for end in candidates where !handled.contains(end.id) {
            if end.finish.at < startedAt || !enabled {
                handled.insert(end.id)
                queued[end.id] = nil
                continue
            }
            let owner = end.start?.session.flatMap { session in agents.first { $0.id == session && Self.canBeWoken($0) } }
            if let owner, case .job(let job) = owner.target {
                if owner.group == .completed {
                    handled.insert(end.id)
                    queued[end.id] = nil
                    actions.append(.send(job: job, text: Self.message(end, lastLine: lastLine(end.finish.log)), id: end.id))
                    continue
                }
                let since = queued[end.id]?.since ?? now
                if now.timeIntervalSince(since) <= Self.queueLimit {
                    queued[end.id] = (end, since)
                    continue
                }
            }
            handled.insert(end.id)
            queued[end.id] = nil
            actions.append(.notify(title: "Run finished", body: Self.summary(end), id: end.id))
        }
        return actions
    }

    /// A background conversation, not a command job.
    private static func canBeWoken(_ agent: Agent) -> Bool {
        if case .job = agent.target { return agent.command == nil }
        return false
    }

    static func message(_ end: RunEnd, lastLine: String?) -> String {
        var text = "Run finished: \(oneLine(RunFormat.truncated(end.key, to: 160))) — exit \(end.finish.rc) after \(RunFormat.duration(end.finish.seconds)). Log: \(end.finish.log)."
        if let lastLine, !lastLine.isEmpty { text += " Last line: \(oneLine(lastLine))." }
        return text
    }

    /// "perf: ok after 32m".
    static func summary(_ end: RunEnd) -> String {
        "\(RunFormat.shortName(end.key)): \(RunFormat.exit(end.finish.rc)) after \(RunFormat.duration(end.finish.seconds))"
    }

    private static func oneLine(_ text: String) -> String {
        text.split(whereSeparator: \.isNewline).joined(separator: " ")
    }
}
