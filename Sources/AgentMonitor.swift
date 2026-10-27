import Foundation

enum AgentGroup: Int, CaseIterable {
    case needsInput, working, completed

    var title: String {
        switch self {
        case .needsInput: return "Needs Input"
        case .working: return "Working"
        case .completed: return "Completed"
        }
    }
}

/// What clicking an agent does.
enum AgentTarget {
    case job(String)                             // background job: `claude attach <id>`
    case process(pid_t)                          // live terminal session: bring its window forward
    case resume(sessionID: String, cwd: String)  // exited session: `claude --resume <id>`
}

struct Agent {
    let id: String
    let name: String
    let project: String
    let cwd: String
    var group: AgentGroup
    var label: String
    let detail: String?
    let since: Date
    let target: AgentTarget
    var suggestedReply: String? = nil
    var transcriptPath: String? = nil
    /// Where it runs: "Background", "claude -p", or the app hosting its terminal.
    var host: String = ""

    /// Replies can be typed in only for background jobs; terminal sessions belong to their window.
    var canReply: Bool {
        if case .job = target { return true }
        return false
    }
}

/// Reads Claude Code's own state files:
///   ~/.claude/sessions/<pid>.json   one per running session, status busy|shell|waiting|idle
///   ~/.claude/jobs/<id>/state.json  one per background job, state working|blocked|done|...
final class AgentMonitor {
    private let claudeDir = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".claude")
    private var lastLive: [String: Agent] = [:]
    private var exited: [String: Agent] = [:]

    /// Completed agents older than this are hidden.
    var completedWindow: TimeInterval = 24 * 3600

    func poll() -> [Agent] {
        let sessions = readSessions()
        let jobs = readJobs()

        var sessionByJob: [String: [String: Any]] = [:]
        for s in sessions {
            if let jobID = s["jobId"] as? String { sessionByJob[jobID] = s }
        }

        var result: [Agent] = []
        for (shortID, job) in jobs {
            result.append(agent(fromJob: job, shortID: shortID, live: sessionByJob[shortID]))
        }

        var live: [String: Agent] = [:]
        for s in sessions {
            if let jobID = s["jobId"] as? String, jobs[jobID] != nil { continue }
            // A terminal session that handed its work to a background job is shown as that job.
            if let parked = s["parkedJobId"] as? String, jobs[parked] != nil { continue }
            guard let a = agent(fromSession: s) else { continue }
            live[a.id] = a
            exited[a.id] = nil
        }

        let jobSessionIDs = Set(result.map(\.id))
        for (id, old) in lastLive where live[id] == nil && !jobSessionIDs.contains(id) {
            exited[id] = Agent(id: id, name: old.name, project: old.project, cwd: old.cwd,
                               group: .completed, label: "Exited", detail: old.detail, since: Date(),
                               target: .resume(sessionID: id, cwd: old.cwd))
        }
        lastLive = live

        result += live.values
        result += exited.values

        let cutoff = Date().addingTimeInterval(-completedWindow)
        return result
            .filter { $0.group != .completed || $0.since > cutoff }
            .sorted { $0.group.rawValue != $1.group.rawValue ? $0.group.rawValue < $1.group.rawValue : $0.since > $1.since }
    }

    // MARK: - Jobs

    private func agent(fromJob job: [String: Any], shortID: String, live: [String: Any]?) -> Agent {
        let state = job["state"] as? String ?? ""
        let tempo = job["tempo"] as? String
        let status = live?["status"] as? String

        let group: AgentGroup
        let label: String
        if status == "busy" || status == "shell" {
            (group, label) = (.working, "Working")
        } else if state == "blocked" || tempo == "blocked" || status == "waiting" {
            (group, label) = (.needsInput, "Needs input")
        } else if state == "working" {
            (group, label) = (.working, "Working")
        } else {
            (group, label) = (.completed, state.isEmpty ? "Idle" : state.capitalized)
        }

        let detail = group == .needsInput
            ? (job["needs"] as? String ?? job["detail"] as? String)
            : job["detail"] as? String
        let cwd = job["cwd"] as? String ?? ""
        var since = (job["updatedAt"] as? String).flatMap(Self.parseISO) ?? .distantPast
        if let ms = live?["statusUpdatedAt"] as? Double {
            since = max(since, Date(timeIntervalSince1970: ms / 1000))
        }

        return Agent(id: job["sessionId"] as? String ?? shortID,
                     name: job["name"] as? String ?? shortID,
                     project: (cwd as NSString).lastPathComponent, cwd: cwd,
                     group: group, label: label, detail: detail, since: since,
                     target: .job(shortID),
                     suggestedReply: job["suggestedReply"] as? String,
                     transcriptPath: job["linkScanPath"] as? String,
                     host: "Background")
    }

    private func readJobs() -> [String: [String: Any]] {
        let dir = claudeDir.appendingPathComponent("jobs")
        let names = (try? FileManager.default.contentsOfDirectory(atPath: dir.path)) ?? []
        var jobs: [String: [String: Any]] = [:]
        for name in names {
            let url = dir.appendingPathComponent(name).appendingPathComponent("state.json")
            if let obj = Self.readJSON(url) { jobs[name] = obj }
        }
        return jobs
    }

    // MARK: - Sessions

    private func agent(fromSession s: [String: Any]) -> Agent? {
        guard let id = s["sessionId"] as? String, let pid = (s["pid"] as? NSNumber)?.int32Value else { return nil }
        let status = s["status"] as? String ?? "idle"
        let cwd = s["cwd"] as? String ?? ""

        let group: AgentGroup
        let label: String
        switch status {
        case "waiting": (group, label) = (.needsInput, "Waiting for you")
        case "busy", "shell": (group, label) = (.working, "Working")
        default: (group, label) = (.completed, "Idle")
        }

        let ms = s["statusUpdatedAt"] as? Double ?? s["updatedAt"] as? Double ?? s["startedAt"] as? Double ?? 0
        let headless = (s["entrypoint"] as? String) == "sdk-cli"
        return Agent(id: id,
                     name: s["name"] as? String ?? (cwd as NSString).lastPathComponent,
                     project: (cwd as NSString).lastPathComponent, cwd: cwd,
                     group: group, label: label,
                     detail: nil,
                     since: Date(timeIntervalSince1970: ms / 1000),
                     target: .process(pid),
                     host: headless ? "claude -p" : AgentActions.hostAppName(of: pid) ?? "Terminal")
    }

    /// Session files whose process is still alive.
    private func readSessions() -> [[String: Any]] {
        let dir = claudeDir.appendingPathComponent("sessions")
        let names = (try? FileManager.default.contentsOfDirectory(atPath: dir.path)) ?? []
        return names.filter { $0.hasSuffix(".json") }.compactMap { name in
            guard let obj = Self.readJSON(dir.appendingPathComponent(name)),
                  let pid = (obj["pid"] as? NSNumber)?.int32Value,
                  Self.isAlive(pid) else { return nil }
            return obj
        }
    }

    // MARK: - Helpers

    private static func readJSON(_ url: URL) -> [String: Any]? {
        guard let data = try? Data(contentsOf: url) else { return nil }
        return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
    }

    private static func isAlive(_ pid: pid_t) -> Bool {
        kill(pid, 0) == 0 || errno == EPERM
    }

    private static let isoFormatter: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    private static func parseISO(_ s: String) -> Date? {
        isoFormatter.date(from: s) ?? ISO8601DateFormatter().date(from: s)
    }
}
