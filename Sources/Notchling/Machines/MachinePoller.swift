import AppKit

/// Asks every worker machine for `worker status --json` once a minute and keeps the answers for the Machines
/// view. It never polls while the Mac sleeps: it stops on sleep, and on wake it refreshes once and starts again.
@MainActor
final class MachinePoller: ObservableObject {
    /// The latest poll of each machine by name: a status, or the error that stopped it.
    @Published private(set) var reports: [String: LastReport] = [:]
    /// The report before the latest one that held a status. A failed poll leaves it as it was, so a run of
    /// failures still shows what the machine was doing before it went quiet (see `MachineHealth.word`).
    @Published private(set) var previous: [String: LastReport] = [:]

    /// Fires once per (machine, repo, issue) the first time it shows in a status, until the app quits. An item
    /// that leaves and comes back does not fire again.
    var onNewNeedsYou: ((Machine, NeedsYouEntry) -> Void)?

    static let interval: TimeInterval = 60
    private static let commandTimeout: TimeInterval = 30

    private let runner: CommandRunning
    private let machines: () -> [Machine]
    private let clock: () -> Date
    private let observeSleep: Bool
    private var timer: Timer?
    private var observers: [NSObjectProtocol] = []
    private var refreshing = false
    private var announced = Set<String>()

    /// - Parameter observeSleep: false in tests, so they never depend on a real sleep or wake.
    init(runner: CommandRunning, machines: @escaping () -> [Machine], clock: @escaping () -> Date,
         observeSleep: Bool = true) {
        self.runner = runner
        self.machines = machines
        self.clock = clock
        self.observeSleep = observeSleep
    }

    deinit {
        // the notification centre is thread safe; the timer is invalidated by `stop()` or dies with the run loop
        for o in observers { NSWorkspace.shared.notificationCenter.removeObserver(o) }
    }

    /// Polls every machine at once. A poll that is still running makes this a no-op.
    func refresh() async {
        guard !refreshing else { return }
        refreshing = true
        defer { refreshing = false }

        let list = machines()
        let runner = runner
        let results = await withTaskGroup(of: (Machine, CommandResult).self) { group in
            for m in list {
                group.addTask {
                    let remote = [m.python, m.sdPath, "worker", "status", "--json"]
                    return (m, await runner.run(SSH.argv(m, remote), cwd: nil, timeout: Self.commandTimeout))
                }
            }
            var all: [(Machine, CommandResult)] = []
            for await r in group { all.append(r) }
            return all
        }

        let now = clock()
        let names = Set(list.map(\.name))
        reports = reports.filter { names.contains($0.key) }
        previous = previous.filter { names.contains($0.key) }
        for (m, result) in results {
            let report = Self.report(from: result, at: now)
            if let old = reports[m.name], old.status != nil { previous[m.name] = old }
            reports[m.name] = report
            if let status = report.status, report.error == nil { announceNew(status.needsYou, on: m) }
        }
    }

    /// Polls now and then every 60 seconds, and follows the Mac's sleep and wake.
    func start() {
        startTimer()
        if observeSleep && observers.isEmpty {
            let center = NSWorkspace.shared.notificationCenter
            observers.append(center.addObserver(forName: NSWorkspace.willSleepNotification, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.stopTimer() }
            })
            observers.append(center.addObserver(forName: NSWorkspace.didWakeNotification, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.startTimer() }
            })
        }
    }

    func stop() {
        stopTimer()
        for o in observers { NSWorkspace.shared.notificationCenter.removeObserver(o) }
        observers = []
    }

    private func startTimer() {
        stopTimer()
        Task { await refresh() }
        let t = Timer(timeInterval: Self.interval, repeats: true) { [weak self] _ in
            Task { @MainActor in await self?.refresh() }
        }
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }

    private func stopTimer() {
        timer?.invalidate()
        timer = nil
    }

    private func announceNew(_ entries: [NeedsYouEntry], on m: Machine) {
        for e in entries where announced.insert("\(m.name)\u{1F}\(e.repo)\u{1F}\(e.issue)").inserted {
            onNewNeedsYou?(m, e)
        }
    }

    private static func report(from result: CommandResult, at now: Date) -> LastReport {
        switch result.code {
        case 0:
            if let status = WorkerStatus.decode(Data(result.out.utf8)) { return LastReport(status: status, at: now) }
            return LastReport(at: now, error: "unreadable status")
        case 124: return LastReport(at: now, error: "timed out")
        case 127: return LastReport(at: now, error: "ssh not found")
        default:
            let text = result.err.trimmingCharacters(in: .whitespacesAndNewlines)
            return LastReport(at: now, error: text.isEmpty ? "ssh exited \(result.code)" : text)
        }
    }
}
