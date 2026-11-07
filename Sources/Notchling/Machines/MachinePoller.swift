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

    /// Fires once for the first good poll of each machine (all machines of one round together) with the number of
    /// questions that were already waiting, instead of announcing them one by one: Notchling relaunches on
    /// every install, and each launch would else ring once per open question. Not called for none.
    var onQuestionsWaiting: ((Int) -> Void)?

    static let interval: TimeInterval = 60
    private static let commandTimeout: TimeInterval = 30

    private let runner: CommandRunning
    private let machines: () -> [Machine]
    private let clock: () -> Date
    private let observeSleep: Bool
    private var timer: Timer?
    private var observers: [NSObjectProtocol] = []
    private var refreshing = false
    /// A refresh asked for while one was running, to run when that one ends (the wake after a sleep).
    private var refreshQueued = false
    private var announced = Set<String>()
    /// Machines with a good poll behind them.
    private var baselined = Set<String>()

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

    /// Polls every machine at once. A poll that is still running makes this a no-op, unless `queueIfBusy`: then
    /// one more poll runs when it ends. The wake from sleep asks that way, because the poll in flight at sleep
    /// may be stuck on a connection that died, and its answer is a stale "unreachable".
    func refresh(queueIfBusy: Bool = false) async {
        guard !refreshing else {
            if queueIfBusy { refreshQueued = true }
            return
        }
        refreshing = true
        defer { refreshing = false }
        repeat {
            refreshQueued = false
            await poll()
        } while refreshQueued
    }

    private func poll() async {
        // a machine without an sd worker job, or whose job has no `sdPath` yet (Plugins installed finds it), has
        // nothing to ask: `python3 '' worker status` would only fail
        let list = machines().filter { $0.sdWorker != nil && !$0.sdPath.isEmpty }
        let runner = runner
        let results = await withTaskGroup(of: (Machine, CommandResult).self) { group in
            for m in list {
                group.addTask {
                    // `RemoteShell.path` lets a `~/…` path expand for the worker user
                    let script = RemoteShell.path(m.python) + " " + RemoteShell.path(m.sdPath) + " worker status --json"
                    return (m, await runner.run(SSH.script(m, script), cwd: nil, timeout: Self.commandTimeout))
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
        baselined = baselined.intersection(names)
        var waiting = 0
        for (m, result) in results {
            let report = Self.report(from: result, at: now)
            if let old = reports[m.name], old.status != nil { previous[m.name] = old }
            reports[m.name] = report
            guard let status = report.status, report.error == nil else { continue }
            if baselined.insert(m.name).inserted {
                waiting += status.needsYou.count
                for e in status.needsYou { announced.insert(Self.key(m, e)) }
            } else {
                announceNew(status.needsYou, on: m)
            }
        }
        if waiting > 0 { onQuestionsWaiting?(waiting) }
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
                MainActor.assumeIsolated { self?.startTimer(afterWake: true) }
            })
        }
    }

    func stop() {
        stopTimer()
        for o in observers { NSWorkspace.shared.notificationCenter.removeObserver(o) }
        observers = []
    }

    private func startTimer(afterWake: Bool = false) {
        stopTimer()
        Task { await refresh(queueIfBusy: afterWake) }
        let t = Timer(timeInterval: Self.interval, repeats: true) { [weak self] timer in
            Self.tick(timer, poller: self)
        }
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }

    /// One tick of the timer. A poller that was freed while its timer was still on the run loop stops the timer.
    nonisolated static func tick(_ timer: Timer, poller: MachinePoller?) {
        guard let poller else {
            timer.invalidate()
            return
        }
        Task { @MainActor in await poller.refresh() }
    }

    private func stopTimer() {
        timer?.invalidate()
        timer = nil
    }

    private func announceNew(_ entries: [NeedsYouEntry], on m: Machine) {
        for e in entries where announced.insert(Self.key(m, e)).inserted {
            onNewNeedsYou?(m, e)
        }
    }

    private static func key(_ m: Machine, _ e: NeedsYouEntry) -> String { "\(m.name)\u{1F}\(e.repo)\u{1F}\(e.issue)" }

    private static func report(from result: CommandResult, at now: Date) -> LastReport {
        switch result.code {
        case 0:
            if let status = WorkerStatus.decode(Data(result.out.utf8)) { return LastReport(status: status, at: now) }
            return LastReport(at: now, error: "unreadable status")
        case 124: return LastReport(at: now, error: "timed out")
        case 127:
            // 127 is also what the shell on the machine says for a missing python or `sd.py`
            let text = result.err.trimmingCharacters(in: .whitespacesAndNewlines)
            if text == "/usr/bin/ssh not found" { return LastReport(at: now, error: "ssh not found") }
            let first = text.split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: true).first.map(String.init) ?? ""
            return LastReport(at: now, error: first.isEmpty ? "not found on the machine" : "not found on the machine: \(first)")
        default:
            let text = result.err.trimmingCharacters(in: .whitespacesAndNewlines)
            return LastReport(at: now, error: text.isEmpty ? "ssh exited \(result.code)" : text)
        }
    }
}
