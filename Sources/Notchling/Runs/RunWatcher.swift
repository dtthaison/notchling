import Foundation

/// One run as the notch shows it.
struct RunItem: Equatable, Identifiable {
    enum Kind { case active, finished }
    var id: String
    var kind: Kind
    var name: String
    /// Right of the name: "32m / 2h · load 3.2", or "ran 32m · 4m ago".
    var trailing: String
    /// Under the name: the log's last line, or how a finished run ended.
    var detail: String
    var log: String
    var rc: Int?
}

/// Finds the long runs sd starts in the repositories Claude works in, shows them, and tells the session
/// that started one when it ends (see `RunWaker`).
///
/// Nothing here polls for runs. FSEvents says when a repo's `<common>/sd/runs.jsonl` grew and when an
/// active run's log changed; `tick` (called with the app's existing refresh) only re-derives what the
/// clock changes, looks for repositories of new sessions and gives a queued wake its chance.
/// Everything runs on the main thread.
final class RunWatcher {
    /// A finished run stays in the Completed tab this long.
    static let finishedShown: TimeInterval = 600
    private static let pathTTL: TimeInterval = 60

    private struct Repo {
        var dir: String
        var tail = JSONLTail()
        var ledger = RunLedger()
        var file: URL { URL(fileURLWithPath: dir).appendingPathComponent("sd/runs.jsonl") }
    }

    // Injected, so a test can drive time, processes and the outside world.
    private let now: () -> Date
    private let pidAlive: (Int32) -> Bool
    private let load: () -> Double?
    private let extraRepos: () -> [String]
    private let enabled: () -> Bool
    private let send: (_ job: String, _ text: String) throws -> Void
    private let notify: (_ title: String, _ body: String) -> Void
    private let dispatch: (@escaping () -> Void) -> Void
    private let watchFiles: Bool

    private var repos: [String: Repo] = [:]
    private var waker: RunWaker
    private var agents: [Agent] = []
    private var commonDirs: [String: (dir: String?, at: Date)] = [:]
    private var extra: (paths: [String], at: Date)?
    private var lastLines: [String: String] = [:]
    private var dismissed = Set<String>()
    private var items: [RunItem] = []
    private let queue = DispatchQueue(label: "local.notchling.runs")
    private var runsEvents: FileEvents?
    private var watchedSD: Set<String> = []
    private var logEvents: FileEvents?
    private var watchedLogFolders: Set<String> = []

    /// Called on the main thread when the items change.
    var onChange: (([RunItem]) -> Void)?
    private(set) var hasActive = false

    init(now: @escaping () -> Date = Date.init,
         pidAlive: @escaping (Int32) -> Bool = RunWatcher.processExists,
         load: @escaping () -> Double? = RunWatcher.loadAverage,
         extraRepos: @escaping () -> [String] = { [] },
         enabled: @escaping () -> Bool = { true },
         send: @escaping (String, String) throws -> Void,
         notify: @escaping (String, String) -> Void,
         dispatch: @escaping (@escaping () -> Void) -> Void = { DispatchQueue.global(qos: .userInitiated).async(execute: $0) },
         watchFiles: Bool = true) {
        self.now = now
        self.pidAlive = pidAlive
        self.load = load
        self.extraRepos = extraRepos
        self.enabled = enabled
        self.send = send
        self.notify = notify
        self.dispatch = dispatch
        self.watchFiles = watchFiles
        waker = RunWaker(startedAt: now())
    }

    func stop() {
        runsEvents = nil
        logEvents = nil
    }

    // MARK: Driving

    /// `agents`: every Claude Code session, hidden ones too (a hidden session can still be woken).
    func tick(agents: [Agent]) {
        self.agents = agents
        let wanted = repoDirs(for: agents)
        var added = false
        for dir in wanted where repos[dir] == nil {
            repos[dir] = Repo(dir: dir)
            read(dir)
            added = true
        }
        let sd = Set(repos.keys.map { $0 + "/sd" }.filter { FileManager.default.fileExists(atPath: $0) })
        if watchFiles, sd != watchedSD {
            watchedSD = sd
            runsEvents = sd.isEmpty ? nil : FileEvents(folders: sd.map { URL(fileURLWithPath: $0) }, latency: 1, queue: queue) { [weak self] batch in
                // Both the runs file and anything else in sd/ (nightly results) land here; only runs.jsonl matters.
                guard batch.rescan || batch.paths.contains(where: { $0.hasSuffix("/runs.jsonl") }) else { return }
                DispatchQueue.main.async { self?.runsFileChanged() }
            }
            // anything appended between the first read and the stream starting
            if !added { readAll() }
        }
        refresh()
    }

    /// A finished run leaves the Completed tab until it's gone for good.
    func clearFinished() {
        for item in items where item.kind == .finished { dismissed.insert(item.id) }
        publish()
    }

    /// FSEvents saw `runs.jsonl` change (or lost track): read what was appended.
    func runsFileChanged() {
        readAll()
        refresh()
    }

    /// A watched log changed: its last line is stale.
    func logChanged(_ paths: Set<String>) {
        var changed = false
        for run in activeRuns() where paths.contains(run.log) {
            lastLines[run.log] = RunLog.lastLine(of: run.log)
            changed = true
        }
        if changed { publish() }
    }

    // MARK: Reading

    private func readAll() {
        for dir in repos.keys { read(dir) }
    }

    private func read(_ dir: String) {
        guard var repo = repos[dir] else { return }
        let result = repo.tail.read(repo.file)
        if result.reset { repo.ledger.reset() }
        repo.ledger.apply(lines: result.lines)
        repo.ledger.prune(now: now())
        repos[dir] = repo
    }

    private func activeRuns() -> [RunStart] {
        let at = now()
        return repos.values.flatMap { $0.ledger.active(now: at, pidAlive: pidAlive) }.sorted { $0.startedAt < $1.startedAt }
    }

    private func recentEnds() -> [RunEnd] {
        let cutoff = now().addingTimeInterval(-Self.finishedShown)
        return repos.values.flatMap { $0.ledger.finished(since: cutoff) }.sorted { $0.finish.at > $1.finish.at }
    }

    /// Repositories (git common dirs) to watch: the sessions' folders, and the checkouts on `machines.json`.
    private func repoDirs(for agents: [Agent]) -> Set<String> {
        let at = now()
        if extra == nil || at.timeIntervalSince(extra!.at) > Self.pathTTL { extra = (extraRepos(), at) }
        var dirs = Set<String>()
        for path in Set(agents.map(\.cwd)).union(extra?.paths ?? []) where !path.isEmpty {
            if let hit = commonDirs[path], at.timeIntervalSince(hit.at) < Self.pathTTL {
                if let dir = hit.dir { dirs.insert(dir) }
                continue
            }
            let dir = GitCommonDir.resolve(cwd: path)
            commonDirs[path] = (dir, at)
            if let dir { dirs.insert(dir) }
        }
        // a repo once seen stays: its runs may outlive the session's row
        return dirs.union(repos.keys)
    }

    // MARK: Deriving

    private func refresh() {
        let active = activeRuns()
        hasActive = !active.isEmpty
        for run in active where lastLines[run.log] == nil { lastLines[run.log] = RunLog.lastLine(of: run.log) }
        watchLogs(of: active)
        wake()
        publish()
    }

    /// One stream over the parent folders of the active logs; latency 1 s is the throttle.
    private func watchLogs(of active: [RunStart]) {
        guard watchFiles else { return }
        let folders = Set(active.map { ($0.log as NSString).deletingLastPathComponent }.filter { FileManager.default.fileExists(atPath: $0) })
        guard folders != watchedLogFolders else { return }
        watchedLogFolders = folders
        let logs = Set(active.map(\.log))
        logEvents = folders.isEmpty ? nil : FileEvents(folders: folders.map { URL(fileURLWithPath: $0) }, latency: 1, queue: queue) { [weak self] batch in
            let hit = batch.rescan ? logs : batch.paths.intersection(logs)
            guard !hit.isEmpty else { return }
            DispatchQueue.main.async { self?.logChanged(hit) }
        }
    }

    private func wake() {
        let actions = waker.plan(ends: recentEnds(), agents: agents, enabled: enabled(), now: now()) { RunLog.lastLine(of: $0) }
        for action in actions {
            switch action {
            case .notify(let title, let body, _):
                notify(title, body)
            case .send(let job, let text, let id):
                let fallback = (title: "Run finished", body: Self.fallbackBody(id: id, ends: recentEnds()))
                dispatch { [send, notify] in
                    do { try send(job, text) } catch {
                        DispatchQueue.main.async { notify(fallback.title, fallback.body + " (couldn't reach its session)") }
                    }
                }
            }
        }
    }

    private static func fallbackBody(id: String, ends: [RunEnd]) -> String {
        ends.first { $0.id == id }.map(RunWaker.summary) ?? "A run finished"
    }

    private func publish() {
        let at = now()
        let load = self.load().map { String(format: "load %.1f", $0) }
        var next: [RunItem] = activeRuns().map { run in
            let progress = RunFormat.progress(elapsed: at.timeIntervalSince(run.startedAt), limit: run.limit)
            return RunItem(id: run.log, kind: .active, name: RunFormat.shortName(run.key),
                           trailing: [progress, load].compactMap { $0 }.joined(separator: " · "),
                           detail: lastLines[run.log] ?? "Starting…", log: run.log, rc: nil)
        }
        for end in recentEnds() where !dismissed.contains(end.id) {
            let ago = at.timeIntervalSince(end.finish.at)
            let how = RunFormat.exit(end.finish.rc)
            let cached = lastLines[end.id] ?? RunLog.lastLine(of: end.finish.log) ?? ""
            lastLines[end.id] = cached
            let line = cached.isEmpty ? nil : cached
            next.append(RunItem(id: end.id, kind: .finished, name: RunFormat.shortName(end.key),
                                trailing: "ran \(RunFormat.duration(end.finish.seconds)) · \(ago < 60 ? "now" : RunFormat.duration(ago))",
                                detail: [how, line].compactMap { $0 }.joined(separator: " · "), log: end.finish.log, rc: end.finish.rc))
        }
        let keep = Set(next.map(\.id)).union(activeRuns().map(\.log))
        lastLines = lastLines.filter { keep.contains($0.key) }
        guard next != items else { return }
        items = next
        onChange?(next)
    }

    // MARK: System

    static func processExists(_ pid: Int32) -> Bool {
        guard pid > 0 else { return false }
        return kill(pid, 0) == 0 || errno == EPERM
    }

    static func loadAverage() -> Double? {
        var load = [Double](repeating: 0, count: 1)
        return getloadavg(&load, 1) == 1 ? load[0] : nil
    }
}
