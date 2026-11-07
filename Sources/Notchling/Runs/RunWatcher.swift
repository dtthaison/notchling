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
    /// Every message to a session goes through here, one at a time: `claude attach` for two jobs at once is two
    /// pseudo-terminals racing for the same `claude` start-up.
    private static let sendQueue = DispatchQueue(label: "local.notchling.runs.send", qos: .userInitiated)

    private struct Repo {
        var dir: String
        var tail = JSONLTail()
        var ledger = RunLedger()
        var file: URL { URL(fileURLWithPath: dir).appendingPathComponent("sd/runs.jsonl") }
    }

    // Injected, so a test can drive time, processes and the outside world.
    private let now: () -> Date
    private let pidAlive: (RunStart) -> Bool
    private let load: () -> Double?
    private let extraRepos: () -> [String]
    private let enabled: () -> Bool
    private let send: (_ job: String, _ text: String) throws -> Void
    private let notify: (_ title: String, _ body: String) -> Void
    private let dispatch: (@escaping () -> Void) -> Void
    private let watchFiles: Bool
    /// Test seam: runs after a repository's first read and before FSEvents listens to `sd/`.
    var beforeWatching: (() -> Void)?

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
    /// Messages waiting for their job, and the jobs with a send in flight (a job never has two).
    private var outbox: [String: (texts: [String], fallbacks: [String])] = [:]
    private var outboxOrder: [String] = []
    private var sending = Set<String>()

    /// Called on the main thread when the items change, and once when set if there already are some.
    var onChange: (([RunItem]) -> Void)? {
        didSet { if !items.isEmpty { onChange?(items) } }
    }
    private(set) var hasActive = false

    init(now: @escaping () -> Date = Date.init,
         pidAlive: @escaping (RunStart) -> Bool = RunWatcher.isRunning,
         load: @escaping () -> Double? = RunWatcher.loadAverage,
         extraRepos: @escaping () -> [String] = { [] },
         enabled: @escaping () -> Bool = { true },
         send: @escaping (String, String) throws -> Void,
         notify: @escaping (String, String) -> Void,
         dispatch: @escaping (@escaping () -> Void) -> Void = { RunWatcher.sendQueue.async(execute: $0) },
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
        for dir in wanted where repos[dir] == nil {
            repos[dir] = Repo(dir: dir)
            read(dir)
        }
        let sd = Set(repos.keys.map { $0 + "/sd" }.filter { FileManager.default.fileExists(atPath: $0) })
        if watchFiles, sd != watchedSD {
            watchedSD = sd
            beforeWatching?()
            runsEvents = sd.isEmpty ? nil : FileEvents(folders: sd.map { URL(fileURLWithPath: $0) }, latency: 1, queue: queue) { [weak self] batch in
                // Both the runs file and anything else in sd/ (nightly results) land here; only runs.jsonl matters.
                guard batch.rescan || batch.paths.contains(where: { $0.hasSuffix("/runs.jsonl") }) else { return }
                DispatchQueue.main.async { self?.runsFileChanged() }
            }
            // anything appended between a repository's first read and the stream starting
            readAll()
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

    /// A watched log changed (`rescan`: FSEvents lost track, so any might have): its last line is stale.
    /// FSEvents names a file as `/var/...` where sd's record says `/private/var/...`, so both sides are resolved.
    func logChanged(_ paths: Set<String>, rescan: Bool = false) {
        let real = Set(paths.map(FileEvents.realPath))
        var changed = false
        for run in activeRuns() where rescan || real.contains(FileEvents.realPath(run.log)) {
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
        reconcile()
        let active = activeRuns()
        hasActive = !active.isEmpty
        for run in active where lastLines[run.log] == nil { lastLines[run.log] = RunLog.lastLine(of: run.log) }
        watchLogs(of: active)
        wake()
        publish()
    }

    /// A run seen alive whose process is gone (or isn't that run any more) and whose finish line hasn't come:
    /// read the file again, since sd may just have written it, and if it still isn't there the run ended
    /// without a result.
    private func reconcile() {
        for dir in Array(repos.keys) {
            guard var repo = repos[dir] else { continue }
            let lost = repo.ledger.disappeared(now: now(), pidAlive: pidAlive)
            repos[dir] = repo
            guard !lost.isEmpty else { continue }
            read(dir)
            repos[dir]?.ledger.markLost(lost, at: now())
        }
    }

    /// One stream over the parent folders of the active logs; latency 1 s is the throttle.
    private func watchLogs(of active: [RunStart]) {
        guard watchFiles else { return }
        let folders = Set(active.map { ($0.log as NSString).deletingLastPathComponent }.filter { FileManager.default.fileExists(atPath: $0) })
        guard folders != watchedLogFolders else { return }
        watchedLogFolders = folders
        // Which logs matter is decided on the main thread against the runs active by then, not the ones
        // active when the stream was made: a run that starts in a folder already watched gets no new stream.
        logEvents = folders.isEmpty ? nil : FileEvents(folders: folders.map { URL(fileURLWithPath: $0) }, latency: 1, queue: queue) { [weak self] batch in
            DispatchQueue.main.async { self?.logChanged(batch.paths, rescan: batch.rescan) }
        }
    }

    private func wake() {
        let actions = waker.plan(ends: recentEnds(), agents: agents, enabled: enabled(), now: now()) { RunLog.lastLine(of: $0) }
        guard !actions.isEmpty else { return }
        let ends = recentEnds()
        for action in actions {
            switch action {
            case .notify(let title, let body, _):
                notify(title, body)
            case .send(let job, let text, let ids):
                if outbox[job] == nil { outboxOrder.append(job) }
                var entry = outbox[job] ?? (texts: [], fallbacks: [])
                entry.texts.append(text)
                entry.fallbacks += ids.map { id in ends.first { $0.id == id }.map(RunWaker.summary) ?? "A run finished" }
                outbox[job] = entry
            }
        }
        flushOutbox()
    }

    /// Sends what waits, to each job that has no send in flight. What piled up meanwhile goes out as one message.
    private func flushOutbox() {
        for job in outboxOrder where !sending.contains(job) {
            guard let entry = outbox[job] else { continue }
            outbox[job] = nil
            sending.insert(job)
            let text = entry.texts.joined(separator: "\n")
            let fallback = entry.fallbacks.joined(separator: "; ")
            dispatch { [weak self, send, notify] in
                var failed = false
                do { try send(job, text) } catch { failed = true }
                DispatchQueue.main.async {
                    if failed { notify("Run finished", fallback + " (couldn't reach its session)") }
                    self?.sending.remove(job)
                    self?.flushOutbox()
                }
            }
        }
        outboxOrder.removeAll { outbox[$0] == nil }
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
        for end in recentEnds() where !dismissed.contains(end.id) && !end.isPrecheck {
            let ago = at.timeIntervalSince(end.finish.at)
            let cached = lastLines[end.id] ?? RunLog.lastLine(of: end.finish.log) ?? ""
            lastLines[end.id] = cached
            next.append(RunItem(id: end.id, kind: .finished, name: RunFormat.shortName(end.key),
                                trailing: "ran \(RunFormat.duration(end.finish.seconds)) · \(ago < 60 ? "now" : RunFormat.duration(ago))",
                                detail: RunFormat.outcome(rc: end.finish.rc, lastLine: cached), log: end.finish.log, rc: end.finish.rc))
        }
        let keep = Set(next.map(\.id)).union(activeRuns().map(\.log))
        lastLines = lastLines.filter { keep.contains($0.key) }
        guard next != items else { return }
        items = next
        onChange?(next)
    }

    // MARK: System

    /// The checkouts on `machines.json` (the MacBook side of each repo). Read-only: `MachineRegistry.load` would
    /// migrate a v1 file, and that belongs to the Machines window.
    static func machineRepositories(at url: URL = MachineRegistry.fileURL) -> [String] {
        MachineRegistry.peek(at: url).flatMap { $0.repos.map(\.localPath) }
    }

    /// "Wake agents when runs finish" (default on).
    static var wakeOnFinish: Bool {
        get { UserDefaults.standard.object(forKey: "wakeOnRunFinish") as? Bool ?? true }
        set { UserDefaults.standard.set(newValue, forKey: "wakeOnRunFinish") }
    }

    static func isRunning(_ run: RunStart) -> Bool {
        run.pid.map { RunProcess.isAlive(pid: $0, startedAt: run.startedAt) } ?? false
    }

    static func loadAverage() -> Double? {
        var load = [Double](repeating: 0, count: 1)
        return getloadavg(&load, 1) == 1 ? load[0] : nil
    }
}
