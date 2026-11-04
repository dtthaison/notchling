import Foundation

private struct SessionFile: Decodable {
    let pid: Int32
    let sessionId: String
    let cwd: String?
    let startedAt: Double?
    let kind: String?
    let entrypoint: String?
    let name: String?
    let status: String?
    let statusUpdatedAt: Double?
    let updatedAt: Double?
    let jobId: String?
    let parkedJobId: String?
}

private struct JobFile: Decodable {
    let sessionId: String?
    let state: String?
    let tempo: String?
    let name: String?
    let cwd: String?
    let createdAt: String?
    let updatedAt: String?
    let linkScanPath: String?
}

/// Reads Claude Code's own state files, read-only:
///   ~/.claude/sessions/<pid>.json   one per running session, status busy|shell|waiting|idle
///   ~/.claude/jobs/<id>/state.json  one per background job, state working|blocked|done|...
/// Call `poll()` from one queue only: it remembers which sessions it saw alive.
public final class SessionSource {
    private let paths: ClaudePaths
    private let now: () -> Date
    private let isAlive: (Int32) -> Bool
    private let locator: TranscriptLocator
    /// True for processes Console started itself (their input goes through Console's own pty).
    private let isOwned: (Int32) -> Bool

    private var lastLive: [String: Session] = [:]
    private var exited: [String: Session] = [:]

    /// Done and exited sessions older than this are hidden.
    public var doneWindow: TimeInterval = 24 * 3600

    public init(paths: ClaudePaths,
                locator: TranscriptLocator,
                now: @escaping () -> Date = Date.init,
                isAlive: @escaping (Int32) -> Bool = isProcessAlive,
                isOwned: @escaping (Int32) -> Bool = { _ in false }) {
        self.isOwned = isOwned
        self.paths = paths
        self.locator = locator
        self.now = now
        self.isAlive = isAlive
    }

    public func poll() -> [Session] {
        let current = now()
        let files = readSessionFiles()
        let jobs = readJobs()
        let liveByJob = Dictionary(files.compactMap { file in file.jobId.map { ($0, file) } },
                                   uniquingKeysWith: { first, _ in first })

        var result: [Session] = jobs.map { shortID, job in
            session(fromJob: job, shortID: shortID, live: liveByJob[shortID])
        }

        var live: [String: Session] = [:]
        for file in files {
            if let jobID = file.jobId, jobs[jobID] != nil { continue }
            // A terminal session that handed its work to a background job shows up as that job.
            if let parked = file.parkedJobId, jobs[parked] != nil { continue }
            // `claude -p`: headless, nobody to watch or reply.
            if file.entrypoint == "sdk-cli" { continue }
            let built = session(fromFile: file)
            live[built.id] = built
            exited[built.id] = nil
        }

        let jobSessionIDs = Set(result.map(\.id))
        for (id, previous) in lastLive where live[id] == nil && !jobSessionIDs.contains(id) {
            var gone = previous
            gone.state = .exited
            gone.statusUpdatedAt = current
            exited[id] = gone
        }
        lastLive = live

        result += live.values
        result += exited.values

        let cutoff = current.addingTimeInterval(-doneWindow)
        let visible = result.filter { ($0.state != .done && $0.state != .exited) || $0.statusUpdatedAt > cutoff }
        exited = exited.filter { $0.value.statusUpdatedAt > cutoff }
        return visible
    }

    // MARK: - Sessions

    /// Session files whose process is still alive.
    private func readSessionFiles() -> [SessionFile] {
        JSONFile.contents(of: paths.sessionsDir)
            .filter { $0.pathExtension == "json" }
            .compactMap { JSONFile.decode(SessionFile.self, at: $0) }
            .filter { isAlive($0.pid) }
    }

    private func session(fromFile file: SessionFile) -> Session {
        let cwd = file.cwd ?? ""
        let state: SessionState
        switch file.status {
        case "waiting": state = .needsInput
        case "busy", "shell": state = .running
        default: state = .done
        }
        let updated = dateFromMilliseconds(file.statusUpdatedAt ?? file.updatedAt ?? file.startedAt) ?? .distantPast
        return Session(
            id: file.sessionId, pid: file.pid,
            name: file.name ?? (cwd as NSString).lastPathComponent,
            project: (cwd as NSString).lastPathComponent, cwd: cwd,
            entrypoint: Entrypoint(raw: file.entrypoint), kind: file.kind ?? "interactive",
            state: state, statusUpdatedAt: updated,
            startedAt: dateFromMilliseconds(file.startedAt) ?? updated,
            transcriptURL: locator.locate(sessionId: file.sessionId, cwd: cwd),
            control: isOwned(file.pid) ? .consolePTY : .viewOnly)
    }

    // MARK: - Jobs

    private func readJobs() -> [String: JobFile] {
        var jobs: [String: JobFile] = [:]
        for directory in JSONFile.contents(of: paths.jobsDir) {
            // jobs/ also holds loose files such as pins.json; only folders are jobs.
            guard (try? directory.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true else { continue }
            if let job = JSONFile.decode(JobFile.self, at: directory.appendingPathComponent("state.json")) {
                jobs[directory.lastPathComponent] = job
            }
        }
        return jobs
    }

    private func session(fromJob job: JobFile, shortID: String, live: SessionFile?) -> Session {
        var state: SessionState
        if live?.status == "busy" || live?.status == "shell" {
            state = .running
        } else if job.state == "blocked" || job.tempo == "blocked" || live?.status == "waiting" {
            state = .needsInput
        } else if job.state == "working" {
            state = .running
        } else {
            state = .done
        }

        let cwd = job.cwd ?? ""
        var updated = job.updatedAt.flatMap(Self.parseISO) ?? .distantPast
        if let liveUpdate = dateFromMilliseconds(live?.statusUpdatedAt) { updated = max(updated, liveUpdate) }
        // A job left "blocked" or "working" with no process for over a day was abandoned, not waiting.
        if live == nil, state != .done, now().timeIntervalSince(updated) > doneWindow { state = .done }
        let id = job.sessionId ?? shortID
        let hint = job.linkScanPath.map { URL(fileURLWithPath: $0) }
        return Session(
            id: id, pid: live?.pid,
            name: job.name ?? shortID,
            project: (cwd as NSString).lastPathComponent, cwd: cwd,
            entrypoint: .job, kind: live?.kind ?? "bg",
            state: state, statusUpdatedAt: updated,
            startedAt: job.createdAt.flatMap(Self.parseISO) ?? updated,
            transcriptURL: locator.locate(sessionId: id, cwd: cwd, hint: hint),
            control: .job(shortID))
    }

    private static func parseISO(_ string: String) -> Date? {
        // Formatters are not thread-safe and this is cheap enough to make per call.
        let fractional = ISO8601DateFormatter()
        fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return fractional.date(from: string) ?? ISO8601DateFormatter().date(from: string)
    }
}
