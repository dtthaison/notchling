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
        XCTAssertEqual(Array(argv.prefix(6)), ["/usr/bin/ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "agent@vps.tail"])
        XCTAssertEqual(argv.count, 7)
        // one remote argument, in which the path with a space and the quote stay single words
        XCTAssertEqual(argv[6], "'python3' '/opt/my sd/sd.py' 'it'\\''s' 'worker' 'status' '--json'")
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

    func testNewNeedsYouFiresOnce() async {
        let runner = FakeRunner { _ in CommandResult(code: 0, out: self.statusJSON(machine: "a", needsYou: [("o/r", 7)]), err: "") }
        let p = poller(runner, [machine("a")])
        var fired: [(String, Int)] = []
        p.onNewNeedsYou = { m, e in fired.append((m.name, e.issue)) }
        await p.refresh()
        await p.refresh()
        XCTAssertEqual(fired.count, 1)
        XCTAssertEqual(fired.first?.0, "a")
        XCTAssertEqual(fired.first?.1, 7)
    }

    func testNewNeedsYouFiresForEachNewEntryAndPerMachine() async {
        var issues = [("o/r", 7)]
        let runner = FakeRunner { _ in CommandResult(code: 0, out: self.statusJSON(machine: "x", needsYou: issues), err: "") }
        let p = poller(runner, [machine("a"), machine("b")])
        var fired = Set<String>()
        p.onNewNeedsYou = { m, e in fired.insert("\(m.name)/\(e.repo)#\(e.issue)") }
        await p.refresh()
        XCTAssertEqual(fired, ["a/o/r#7", "b/o/r#7"])
        issues = [("o/r", 7), ("o/r", 8)]
        await p.refresh()
        XCTAssertEqual(fired, ["a/o/r#7", "b/o/r#7", "a/o/r#8", "b/o/r#8"])
    }

    func testEntryThatLeavesAndComesBackDoesNotFireAgain() async {
        var issues = [("o/r", 7)]
        let runner = FakeRunner { _ in CommandResult(code: 0, out: self.statusJSON(machine: "a", needsYou: issues), err: "") }
        let p = poller(runner, [machine("a")])
        var count = 0
        p.onNewNeedsYou = { _, _ in count += 1 }
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
