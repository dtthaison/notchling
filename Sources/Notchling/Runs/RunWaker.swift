import Foundation

/// Decides, for each run that ended, who hears of it. It only decides: the watcher sends and notifies,
/// so this runs the same in a test.
///
/// The session that started a run gets one message, if it's a background conversation (the only kind
/// Notchling can type into). Anything else, such as a terminal session or `claude -p`, gets a notification.
///
/// sd sessions normally wait for a run in a way that wakes them on its own (Monitor, a background command,
/// `sd.py wait`), so a message from here would be a second wake. It is sent only when the session has not
/// moved since before the run ended: it must still be idle 60 s after the finish, and idle since before it.
/// A session that worked at or after the finish already heard.
struct RunWaker {
    enum Action: Equatable {
        /// `claude attach <job>` and type `text`: one line per run in `ids`.
        case send(job: String, text: String, ids: [String])
        case notify(title: String, body: String, id: String)

        var ids: [String] {
            switch self {
            case .send(_, _, let ids): return ids
            case .notify(_, _, let id): return [id]
            }
        }
    }

    /// The owner gets this long after the finish to wake itself.
    static let grace: TimeInterval = 60

    /// An owner that keeps waiting on a prompt this long is told about the run by notification instead.
    static let queueLimit: TimeInterval = 6 * 3600

    /// Runs that ended before this are history: never announced.
    let startedAt: Date
    private(set) var handled = Set<String>()
    private var queued: [String: (end: RunEnd, since: Date)] = [:]
    /// When each agent was first seen idle, for an agent whose state files give no time of their own.
    private var idleSeen: [String: Date] = [:]

    init(startedAt: Date) { self.startedAt = startedAt }

    var queuedCount: Int { queued.count }

    /// `ends` are the recently finished runs; `agents` are all the Claude Code sessions, hidden ones too.
    mutating func plan(ends: [RunEnd], agents: [Agent], enabled: Bool, now: Date, lastLine: (String) -> String?) -> [Action] {
        for agent in agents {
            if agent.group == .completed { if idleSeen[agent.id] == nil { idleSeen[agent.id] = now } } else { idleSeen[agent.id] = nil }
        }
        var candidates = ends
        for (id, entry) in queued where !ends.contains(where: { $0.id == id }) { candidates.append(entry.end) }
        candidates.sort { $0.finish.at < $1.finish.at }

        var actions: [Action] = []
        var sends: [(job: String, lines: [String], ids: [String])] = []
        for end in candidates where !handled.contains(end.id) {
            // a finish with no start (sd's `precheck:` records, lines from before sd wrote starts) has no owner
            guard end.finish.at >= startedAt, enabled, end.start != nil else { done(end); continue }
            let owner = end.start?.session.flatMap { session in agents.first { $0.id == session && Self.canBeWoken($0) } }
            guard let owner, case .job(let job) = owner.target else {
                done(end)
                actions.append(.notify(title: "Run finished", body: Self.summary(end), id: end.id))
                continue
            }
            let since = queued[end.id]?.since ?? now
            queued[end.id] = (end, since)
            switch owner.group {
            case .working:
                // it was working at or after the finish: whatever it waited on woke it
                if now >= end.finish.at { done(end) }
            case .needsInput:
                // typing into a prompt could answer it
                if now.timeIntervalSince(since) > Self.queueLimit {
                    done(end)
                    actions.append(.notify(title: "Run finished", body: Self.summary(end), id: end.id))
                }
            case .completed:
                guard now >= end.finish.at.addingTimeInterval(Self.grace) else { continue }
                done(end)
                guard idleSince(owner, now: now) <= end.finish.at else { continue }
                let line = Self.message(end, lastLine: lastLine(end.finish.log))
                if let i = sends.firstIndex(where: { $0.job == job }) {
                    sends[i].lines.append(line)
                    sends[i].ids.append(end.id)
                } else {
                    sends.append((job, [line], [end.id]))
                }
            }
        }
        return actions + sends.map { .send(job: $0.job, text: $0.lines.joined(separator: "\n"), ids: $0.ids) }
    }

    private mutating func done(_ end: RunEnd) {
        handled.insert(end.id)
        queued[end.id] = nil
    }

    /// Since when the agent has been idle: its own state time, else when it was first seen idle.
    private func idleSince(_ agent: Agent, now: Date) -> Date {
        agent.since > .distantPast ? agent.since : idleSeen[agent.id] ?? now
    }

    /// A background conversation, not a command job.
    private static func canBeWoken(_ agent: Agent) -> Bool {
        if case .job = agent.target { return agent.command == nil }
        return false
    }

    static func message(_ end: RunEnd, lastLine: String?) -> String {
        let key = oneLine(RunFormat.truncated(end.key, to: 160))
        let line = lastLine.flatMap { $0.isEmpty ? nil : oneLine($0) }
        let after = RunFormat.duration(end.finish.seconds)
        guard let rc = end.finish.rc else {
            return "Run finished: \(key) after \(after) — \(RunFormat.outcome(rc: nil, lastLine: line)). Log: \(end.finish.log)."
        }
        var text = "Run finished: \(key) — exit \(rc) after \(after). Log: \(end.finish.log)."
        if let line { text += " Last line: \(line)." }
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
