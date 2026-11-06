import XCTest
@testable import Notchling

/// Records every argv and answers from a closure, so no test starts ssh.
private final class FakeRunner: CommandRunning, @unchecked Sendable {
    private let lock = NSLock()
    private var _calls: [[String]] = []
    var answer: ([String]) -> CommandResult

    init(_ answer: @escaping ([String]) -> CommandResult = { _ in CommandResult(code: 0, out: "{}", err: "") }) {
        self.answer = answer
    }

    var calls: [[String]] { lock.withLock { _calls } }

    private func record(_ argv: [String]) { lock.withLock { _calls.append(argv) } }

    func run(_ argv: [String], cwd: String?, timeout: TimeInterval) async -> CommandResult {
        record(argv)
        return answer(argv)
    }
}

/// Answers with a status at once, except while `hold` is set: then each call waits for `release()`.
private final class HoldingRunner: CommandRunning, @unchecked Sendable {
    private let lock = NSLock()
    private var _calls = 0
    private var held = true
    private var waiting: [CheckedContinuation<Void, Never>] = []
    let answer: String
    init(answer: String) { self.answer = answer }

    var calls: Int { lock.withLock { _calls } }

    func run(_ argv: [String], cwd: String?, timeout: TimeInterval) async -> CommandResult {
        let hold = lock.withLock { () -> Bool in _calls += 1; return _calls == 1 && held }
        if hold {
            await withCheckedContinuation { c in
                lock.withLock { waiting.append(c) }
            }
        }
        return CommandResult(code: 0, out: answer, err: "")
    }

    func release() {
        let list = lock.withLock { () -> [CheckedContinuation<Void, Never>] in
            held = false
            defer { waiting = [] }
            return waiting
        }
        list.forEach { $0.resume() }
    }
}

@MainActor
final class MachinesPollerTests: XCTestCase {
    private let now = Date(timeIntervalSince1970: 1_800_000_000)

    private func machine(_ name: String, user: String = "agent", fileVault: Bool = false) -> Machine {
        Machine(name: name, host: "\(name).tail", user: user, sdPath: "/opt/sd dir/sd.py", fileVault: fileVault, repos: [])
    }

    private func statusJSON(machine: String, needsYou: [(String, Int)] = [], booted: String? = nil) -> String {
        let items = needsYou.map { "{\"repo\":\"\($0.0)\",\"issue\":\($0.1),\"reason\":\"question\"}" }.joined(separator: ",")
        let boot = booted.map { ",\"booted_at\":\"\($0)\"" } ?? ""
        return "{\"machine\":\"\(machine)\",\"version\":\"1.19.1\",\"needs_you\":[\(items)]\(boot)}"
    }

    private func poller(_ runner: FakeRunner, _ machines: [Machine]) -> MachinePoller {
        MachinePoller(runner: runner, machines: { machines }, clock: { self.now }, observeSleep: false)
    }

    // MARK: SSH

    func testSSHArgvQuotesRemote() {
        let m = machine("vps", user: "agent")
        let argv = SSH.argv(m, ["python3", "/opt/my sd/sd.py", "it's", "worker", "status", "--json"])
        // `--` ends the options, so a host written as `-oProxyCommand=…` is a destination, never an option
        XCTAssertEqual(Array(argv.prefix(7)), ["/usr/bin/ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "--", "agent@vps.tail"])
        XCTAssertEqual(argv.count, 8)
        // one remote argument, in which the path with a space and the quote stay single words
        XCTAssertEqual(argv[7], "'python3' '/opt/my sd/sd.py' 'it'\\''s' 'worker' 'status' '--json'")
    }

    func testGitURL() {
        XCTAssertEqual(SSH.gitURL(machine("vps"), path: "/home/agent/repo.git"), "ssh://agent@vps.tail/home/agent/repo.git")
    }

    // MARK: refresh

    func testRefreshRunsStatusCommandOnEveryMachine() async {
        let runner = FakeRunner()
        let p = poller(runner, [machine("a"), machine("b")])
        await p.refresh()
        let calls = runner.calls
        XCTAssertEqual(calls.count, 2)
        for name in ["a", "b"] {
            let call = calls.first { $0.contains("agent@\(name).tail") }
            XCTAssertEqual(call?.last, "'python3' '/opt/sd dir/sd.py' 'worker' 'status' '--json'")
        }
    }

    func testRefreshStoresStatusAndError() async {
        let runner = FakeRunner { argv in
            argv.contains("agent@bad.tail")
                ? CommandResult(code: 255, out: "", err: "ssh: connect to host bad.tail: Operation timed out\n")
                : CommandResult(code: 0, out: self.statusJSON(machine: "good"), err: "")
        }
        let p = poller(runner, [machine("good"), machine("bad")])
        await p.refresh()
        XCTAssertEqual(p.reports["good"]?.status?.machine, "good")
        XCTAssertNil(p.reports["good"]?.error)
        XCTAssertEqual(p.reports["good"]?.at, now)
        XCTAssertNil(p.reports["bad"]?.status)
        XCTAssertEqual(p.reports["bad"]?.error, "ssh: connect to host bad.tail: Operation timed out")
        XCTAssertEqual(p.reports["bad"]?.at, now)
    }

    func testUnreadableOutputIsAnError() async {
        let runner = FakeRunner { _ in CommandResult(code: 0, out: "not json", err: "") }
        let p = poller(runner, [machine("a")])
        await p.refresh()
        XCTAssertNil(p.reports["a"]?.status)
        XCTAssertNotNil(p.reports["a"]?.error)
    }

    func testTimeoutAndMissingSSHHaveReadableErrors() async {
        let runner = FakeRunner { argv in
            argv.contains("agent@slow.tail") ? CommandResult(code: 124, out: "", err: "") : CommandResult(code: 127, out: "", err: "")
        }
        let p = poller(runner, [machine("slow"), machine("gone")])
        await p.refresh()
        XCTAssertEqual(p.reports["slow"]?.error, "timed out")
        XCTAssertEqual(p.reports["gone"]?.error, "ssh not found")
    }

    func testMachineRemovedFromRegistryIsDropped() async {
        let runner = FakeRunner { argv in CommandResult(code: 0, out: self.statusJSON(machine: "x"), err: "") }
        var list = [machine("a"), machine("b")]
        let p = MachinePoller(runner: runner, machines: { list }, clock: { self.now }, observeSleep: false)
        await p.refresh()
        list = [machine("a")]
        await p.refresh()
        XCTAssertNotNil(p.reports["a"])
        XCTAssertNil(p.reports["b"])
        XCTAssertNil(p.previous["b"])
    }

    // MARK: previous

    func testPreviousHoldsTheReportBeforeTheLastOne() async {
        var boot = "2026-11-05T01:00:00Z"
        let runner = FakeRunner { _ in CommandResult(code: 0, out: self.statusJSON(machine: "a", booted: boot), err: "") }
        let p = poller(runner, [machine("a")])
        await p.refresh()
        XCTAssertNil(p.previous["a"])
        boot = "2026-11-06T01:00:00Z"
        await p.refresh()
        XCTAssertEqual(p.previous["a"]?.status?.bootedAt, StatusDates.parse("2026-11-05T01:00:00Z"))
        XCTAssertEqual(p.reports["a"]?.status?.bootedAt, StatusDates.parse("2026-11-06T01:00:00Z"))
    }

    func testFailedRefreshKeepsTheOldPreviousAndNeverStoresAnErrorThere() async {
        var fail = false
        let runner = FakeRunner { _ in
            fail ? CommandResult(code: 255, out: "", err: "down")
                 : CommandResult(code: 0, out: self.statusJSON(machine: "a", needsYou: [("o/r", 1)]), err: "")
        }
        let p = poller(runner, [machine("a")])
        await p.refresh()                      // good
        fail = true
        await p.refresh()                      // first failure: the good report becomes previous
        XCTAssertEqual(p.reports["a"]?.error, "down")
        XCTAssertEqual(p.previous["a"]?.status?.needsYou.count, 1)
        XCTAssertNil(p.previous["a"]?.error)
        await p.refresh()                      // second failure: previous must stay the good report
        XCTAssertEqual(p.previous["a"]?.status?.needsYou.count, 1)
        XCTAssertNil(p.previous["a"]?.error)
        XCTAssertEqual(p.reports["a"]?.error, "down")
    }

    func testFirstFailureLeavesNoPrevious() async {
        let p = poller(FakeRunner { _ in CommandResult(code: 255, out: "", err: "down") }, [machine("a")])
        await p.refresh()
        await p.refresh()
        XCTAssertNil(p.previous["a"])
    }

    // MARK: onNewNeedsYou

    // (The first good poll of a machine only takes stock: see "first poll" below. These start from an empty one.)

    func testNewNeedsYouFiresOnce() async {
        var issues: [(String, Int)] = []
        let runner = FakeRunner { _ in CommandResult(code: 0, out: self.statusJSON(machine: "a", needsYou: issues), err: "") }
        let p = poller(runner, [machine("a")])
        var fired: [(String, Int)] = []
        p.onNewNeedsYou = { m, e in fired.append((m.name, e.issue)) }
        await p.refresh()
        issues = [("o/r", 7)]
        await p.refresh()
        await p.refresh()
        XCTAssertEqual(fired.count, 1)
        XCTAssertEqual(fired.first?.0, "a")
        XCTAssertEqual(fired.first?.1, 7)
    }

    func testNewNeedsYouFiresForEachNewEntryAndPerMachine() async {
        var issues: [(String, Int)] = []
        let runner = FakeRunner { _ in CommandResult(code: 0, out: self.statusJSON(machine: "x", needsYou: issues), err: "") }
        let p = poller(runner, [machine("a"), machine("b")])
        var fired = Set<String>()
        p.onNewNeedsYou = { m, e in fired.insert("\(m.name)/\(e.repo)#\(e.issue)") }
        await p.refresh()
        issues = [("o/r", 7)]
        await p.refresh()
        XCTAssertEqual(fired, ["a/o/r#7", "b/o/r#7"])
        issues = [("o/r", 7), ("o/r", 8)]
        await p.refresh()
        XCTAssertEqual(fired, ["a/o/r#7", "b/o/r#7", "a/o/r#8", "b/o/r#8"])
    }

    func testEntryThatLeavesAndComesBackDoesNotFireAgain() async {
        var issues: [(String, Int)] = []
        let runner = FakeRunner { _ in CommandResult(code: 0, out: self.statusJSON(machine: "a", needsYou: issues), err: "") }
        let p = poller(runner, [machine("a")])
        var count = 0
        p.onNewNeedsYou = { _, _ in count += 1 }
        await p.refresh()
        issues = [("o/r", 7)]
        await p.refresh()
        issues = []
        await p.refresh()
        issues = [("o/r", 7)]
        await p.refresh()
        XCTAssertEqual(count, 1)
    }

    func testFailedPollFiresNothing() async {
        let p = poller(FakeRunner { _ in CommandResult(code: 255, out: "", err: "down") }, [machine("a")])
        var count = 0
        p.onNewNeedsYou = { _, _ in count += 1 }
        await p.refresh()
        XCTAssertEqual(count, 0)
    }

    // MARK: first poll

    /// Notchling relaunches on every install; the questions already waiting must not each ring.
    func testTheFirstGoodPollOfAMachineGivesOneSummaryAndNoSingleAnnouncements() async {
        var issues = [("o/r", 7), ("o/r", 8), ("o/r", 9)]
        let runner = FakeRunner { _ in CommandResult(code: 0, out: self.statusJSON(machine: "a", needsYou: issues), err: "") }
        let p = poller(runner, [machine("a")])
        var single: [Int] = []
        var summaries: [Int] = []
        p.onNewNeedsYou = { _, e in single.append(e.issue) }
        p.onQuestionsWaiting = { summaries.append($0) }
        await p.refresh()
        XCTAssertEqual(single, [], "no question rang one by one on the first poll")
        XCTAssertEqual(summaries, [3])

        await p.refresh()
        XCTAssertEqual(summaries, [3], "the same questions are not summarised again")
        issues.append(("o/r", 10))
        await p.refresh()
        XCTAssertEqual(single, [10], "a question that arrives later is announced on its own")
        XCTAssertEqual(summaries, [3])
    }

    func testTheSummaryCountsEveryMachineOnce() async {
        let runner = FakeRunner { argv in
            let name = argv.contains { $0.contains("a.tail") } ? "a" : "b"
            return CommandResult(code: 0, out: self.statusJSON(machine: name, needsYou: name == "a" ? [("o/r", 1), ("o/r", 2)] : [("o/r", 1)]), err: "")
        }
        let p = poller(runner, [machine("a"), machine("b")])
        var summaries: [Int] = []
        p.onQuestionsWaiting = { summaries.append($0) }
        await p.refresh()
        XCTAssertEqual(summaries, [3], "one notification for the whole first round")
    }

    func testNoQuestionsMeansNoSummary() async {
        let p = poller(FakeRunner { _ in CommandResult(code: 0, out: self.statusJSON(machine: "a"), err: "") }, [machine("a")])
        var summaries: [Int] = []
        p.onQuestionsWaiting = { summaries.append($0) }
        await p.refresh()
        await p.refresh()
        XCTAssertEqual(summaries, [])
    }

    /// A machine that was away on the first round gets its own first poll when it shows up.
    func testAMachineThatAnswersLaterHasItsOwnFirstPoll() async {
        var up = false
        let runner = FakeRunner { _ in
            up ? CommandResult(code: 0, out: self.statusJSON(machine: "a", needsYou: [("o/r", 5), ("o/r", 6)]), err: "")
               : CommandResult(code: 255, out: "", err: "down")
        }
        let p = poller(runner, [machine("a")])
        var single = 0
        var summaries: [Int] = []
        p.onNewNeedsYou = { _, _ in single += 1 }
        p.onQuestionsWaiting = { summaries.append($0) }
        await p.refresh()
        up = true
        await p.refresh()
        XCTAssertEqual(single, 0)
        XCTAssertEqual(summaries, [2])
    }

    func testTheSummaryWords() {
        XCTAssertEqual(MachineAlert.waiting(count: 1), "1 question is waiting")
        XCTAssertEqual(MachineAlert.waiting(count: 4), "4 questions are waiting")
    }

    // MARK: refresh while one runs

    func testAPollThatWasRunningAtSleepDoesNotSwallowTheWakeRefresh() async {
        let runner = HoldingRunner(answer: statusJSON(machine: "a"))
        let p = MachinePoller(runner: runner, machines: { [self] in [machine("a")] }, clock: { self.now }, observeSleep: false)
        let inFlight = Task { await p.refresh() }
        while runner.calls == 0 { await Task.yield() }

        await p.refresh()                       // an ordinary tick while one runs is still a no-op
        XCTAssertEqual(runner.calls, 1)
        await p.refresh(queueIfBusy: true)      // the wake: it must be remembered, not dropped
        XCTAssertEqual(runner.calls, 1, "it does not run beside the poll in flight")

        runner.release()
        await inFlight.value
        XCTAssertEqual(runner.calls, 2, "the poll ran again once the first ended")
        XCTAssertNotNil(p.reports["a"]?.status)
    }

    func testAQueuedRefreshRunsOnlyOnce() async {
        let runner = HoldingRunner(answer: statusJSON(machine: "a"))
        let p = MachinePoller(runner: runner, machines: { [self] in [machine("a")] }, clock: { self.now }, observeSleep: false)
        let inFlight = Task { await p.refresh() }
        while runner.calls == 0 { await Task.yield() }
        await p.refresh(queueIfBusy: true)
        await p.refresh(queueIfBusy: true)
        runner.release()
        await inFlight.value
        XCTAssertEqual(runner.calls, 2)
    }

    // MARK: ProcessRunner (local, harmless commands only)

    func testProcessRunnerRunsAndCapturesOutput() async {
        let r = await ProcessRunner().run(["/bin/sh", "-c", "echo hi; echo oops >&2; exit 3"], cwd: nil, timeout: 5)
        XCTAssertEqual(r.code, 3)
        XCTAssertEqual(r.out, "hi\n")
        XCTAssertEqual(r.err, "oops\n")
    }

    func testProcessRunnerMissingExecutableIs127() async {
        let r = await ProcessRunner().run(["/nonexistent/definitely-not-here"], cwd: nil, timeout: 5)
        XCTAssertEqual(r.code, 127)
    }

    func testProcessRunnerTimeoutIs124() async {
        let start = Date()
        let r = await ProcessRunner().run(["/bin/sleep", "10"], cwd: nil, timeout: 0.3)
        XCTAssertEqual(r.code, 124)
        XCTAssertLessThan(Date().timeIntervalSince(start), 5)
    }

    func testProcessRunnerHandlesLargeOutput() async {
        let r = await ProcessRunner().run(["/bin/sh", "-c", "head -c 300000 /dev/zero | tr '\\0' x"], cwd: nil, timeout: 10)
        XCTAssertEqual(r.code, 0)
        XCTAssertEqual(r.out.count, 300000)
    }
}
