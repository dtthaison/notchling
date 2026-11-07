import XCTest
@testable import Notchling

/// The watcher end to end on a temp repository: files in, items and messages out. FSEvents is off; the
/// tests call what an event would.
final class RunWatcherTests: XCTestCase {
    var root: URL!
    var repo: URL!
    var clock = Date()
    var alive = true
    var sent: [(job: String, text: String)] = []
    var notices: [(title: String, body: String)] = []
    var sendError: Error?
    var enabled = true
    var items: [RunItem] = []

    override func setUp() {
        root = URL(fileURLWithPath: FileManager.default.temporaryDirectory.path).appendingPathComponent("runwatcher-\(UUID().uuidString)")
        repo = root.appendingPathComponent("repo")
        try? FileManager.default.createDirectory(at: repo.appendingPathComponent(".git/sd"), withIntermediateDirectories: true)
        try? FileManager.default.createDirectory(at: root.appendingPathComponent("logs"), withIntermediateDirectories: true)
        clock = Date()
    }

    override func tearDown() { try? FileManager.default.removeItem(at: root) }

    var runsFile: URL { repo.appendingPathComponent(".git/sd/runs.jsonl") }
    var log: String { root.appendingPathComponent("logs/n-1.log").path }

    func makeWatcher(extra: [String] = [], watchFiles: Bool = false) -> RunWatcher {
        let w = RunWatcher(now: { self.clock }, pidAlive: { _ in self.alive }, load: { 2.5 }, extraRepos: { extra },
                           enabled: { self.enabled },
                           send: { job, text in
                               if let e = self.sendError { throw e }
                               self.sent.append((job, text))
                           },
                           notify: { self.notices.append(($0, $1)) },
                           dispatch: { $0() }, watchFiles: watchFiles)
        w.onChange = { self.items = $0 }
        return w
    }

    /// `since`: when the agent last changed state; a day ago unless a test says otherwise (so it is idle since before any finish).
    func agent(_ id: String, _ group: AgentGroup, cwd: String? = nil, since: Date? = nil) -> Agent {
        Agent(id: id, name: id, project: "repo", cwd: cwd ?? repo.path, group: group, label: "", detail: nil,
              since: since ?? clock.addingTimeInterval(-86400), target: .job("job-\(id)"))
    }

    /// Lets the watcher's hop back to the main queue run.
    func settle() {
        let done = expectation(description: "main queue drained")
        DispatchQueue.main.async { done.fulfill() }
        wait(for: [done], timeout: 2)
    }

    func iso(_ d: Date) -> String { ISO8601DateFormatter().string(from: d) }

    func append(_ text: String) {
        if !FileManager.default.fileExists(atPath: runsFile.path) { try? "".write(to: runsFile, atomically: true, encoding: .utf8) }
        let h = try! FileHandle(forWritingTo: runsFile)
        h.seekToEndOfFile()
        h.write(text.data(using: .utf8)!)
        try? h.close()
    }

    func appendStart(session: String = "s1", limit: Int? = 7200, startedAt: Date? = nil) {
        let l = limit.map(String.init) ?? "null"
        append(#"{"event":"start","key":"caffeinate -i just perf suite=x","log":"\#(log)","cwd":"\#(repo.path)","pid":99,"session":"\#(session)","limit":\#(l),"started_at":"\#(iso(startedAt ?? clock))"}"# + "\n")
    }

    func appendFinish(rc: Int = 0, seconds: Double = 120, at: Date? = nil) {
        append(#"{"key":"caffeinate -i just perf suite=x","cmd":"x","seconds":\#(seconds),"rc":\#(rc),"head":"h","clean":true,"log":"\#(log)","at":"\#(iso(at ?? clock))"}"# + "\n")
    }

    func writeLog(_ text: String) { try? text.write(toFile: log, atomically: true, encoding: .utf8) }

    func testAnActiveRunShowsWithElapsedLimitLoadAndLastLine() {
        writeLog("compiling\nrunning 4/10\n")
        appendStart()
        let w = makeWatcher()
        clock = clock.addingTimeInterval(32 * 60)
        w.tick(agents: [agent("s1", .working)])
        XCTAssertTrue(w.hasActive)
        XCTAssertEqual(items.count, 1)
        XCTAssertEqual(items[0].kind, .active)
        XCTAssertEqual(items[0].name, "perf")
        XCTAssertEqual(items[0].trailing, "32m / 2h · load 2.5")
        XCTAssertEqual(items[0].detail, "running 4/10")
    }

    func testNoLimitShowsJustElapsed() {
        appendStart(limit: nil)
        let w = makeWatcher()
        clock = clock.addingTimeInterval(90)
        w.tick(agents: [agent("s1", .working)])
        XCTAssertEqual(items[0].trailing, "1m · load 2.5")
    }

    func testLogChangeRefreshesTheLastLine() {
        writeLog("one\n")
        appendStart()
        let w = makeWatcher()
        w.tick(agents: [agent("s1", .working)])
        XCTAssertEqual(items[0].detail, "one")
        writeLog("one\ntwo\n")
        w.logChanged([log])
        XCTAssertEqual(items[0].detail, "two")
    }

    func testADeadProcessIsNotActive() {
        appendStart()
        alive = false
        let w = makeWatcher()
        w.tick(agents: [agent("s1", .working)])
        XCTAssertFalse(w.hasActive)
        XCTAssertTrue(items.isEmpty)
    }

    func testFinishMovesTheRunToCompletedAndWakesTheSessionOnce() {
        appendStart()
        let w = makeWatcher()
        w.tick(agents: [agent("s1", .completed)])
        writeLog("PASS all\n")
        clock = clock.addingTimeInterval(120)
        appendFinish(rc: 0, seconds: 120, at: clock)
        w.runsFileChanged()
        w.tick(agents: [agent("s1", .completed)])
        XCTAssertFalse(w.hasActive)
        XCTAssertEqual(items.count, 1)
        XCTAssertEqual(items[0].kind, .finished)
        XCTAssertEqual(items[0].rc, 0)
        XCTAssertTrue(items[0].detail.hasPrefix("ok"))
        XCTAssertTrue(sent.isEmpty, "the session gets a minute to wake itself")
        clock = clock.addingTimeInterval(RunWaker.grace + 1)
        w.tick(agents: [agent("s1", .completed)])
        XCTAssertEqual(sent.count, 1)
        XCTAssertEqual(sent[0].job, "job-s1")
        XCTAssertTrue(sent[0].text.hasPrefix("Run finished: caffeinate -i just perf suite=x — exit 0 after 2m. Log: \(log)."))
        XCTAssertTrue(sent[0].text.hasSuffix("Last line: PASS all."))
        w.tick(agents: [agent("s1", .completed)])
        w.runsFileChanged()
        XCTAssertEqual(sent.count, 1)
        XCTAssertTrue(notices.isEmpty)
    }

    func testASessionThatWorkedAfterTheFinishIsNotWoken() {
        appendStart()
        let w = makeWatcher()
        w.tick(agents: [agent("s1", .working)])
        clock = clock.addingTimeInterval(60)
        let finished = clock
        appendFinish(at: finished)
        w.runsFileChanged()
        clock = clock.addingTimeInterval(10)
        w.tick(agents: [agent("s1", .working, since: finished.addingTimeInterval(5))])
        clock = clock.addingTimeInterval(120)
        w.tick(agents: [agent("s1", .completed, since: finished.addingTimeInterval(30))])
        clock = clock.addingTimeInterval(120)
        w.tick(agents: [agent("s1", .completed, since: finished.addingTimeInterval(30))])
        XCTAssertTrue(sent.isEmpty, "waiting in Monitor or `sd.py wait` woke it already")
        XCTAssertTrue(notices.isEmpty)
        XCTAssertEqual(items.count, 1, "the Completed tab still shows it")
    }

    func testNoBackgroundJobPostsANotification() {
        appendStart(session: "terminal-session")
        let w = makeWatcher()
        w.tick(agents: [agent("s1", .completed)])
        clock = clock.addingTimeInterval(60)
        appendFinish(rc: 124, at: clock)
        w.runsFileChanged()
        XCTAssertTrue(sent.isEmpty)
        XCTAssertEqual(notices.count, 1)
        XCTAssertTrue(notices[0].body.contains("stopped at limit"))
        XCTAssertEqual(items[0].detail.prefix(16), "stopped at limit")
    }

    func testAFailedSendFallsBackToANotification() {
        struct Boom: Error {}
        sendError = Boom()
        appendStart()
        let w = makeWatcher()
        w.tick(agents: [agent("s1", .completed)])
        clock = clock.addingTimeInterval(60)
        appendFinish(at: clock)
        w.runsFileChanged()
        clock = clock.addingTimeInterval(RunWaker.grace + 1)
        w.tick(agents: [agent("s1", .completed)])
        XCTAssertTrue(sent.isEmpty)
        let expectation = expectation(description: "notified")
        DispatchQueue.main.async { expectation.fulfill() }
        wait(for: [expectation], timeout: 2)
        XCTAssertEqual(notices.count, 1)
    }

    func testSettingOffNeitherWakesNorNotifies() {
        enabled = false
        appendStart()
        let w = makeWatcher()
        w.tick(agents: [agent("s1", .completed)])
        clock = clock.addingTimeInterval(60)
        appendFinish(at: clock)
        w.runsFileChanged()
        XCTAssertTrue(sent.isEmpty)
        XCTAssertTrue(notices.isEmpty)
        XCTAssertEqual(items.count, 1, "the Completed tab still shows it")
    }

    func testARunThatEndedBeforeNotchlingStartedIsShownButNotAnnounced() {
        appendStart(startedAt: clock.addingTimeInterval(-600))
        appendFinish(at: clock.addingTimeInterval(-300))
        let w = makeWatcher()
        w.tick(agents: [agent("s1", .completed)])
        XCTAssertEqual(items.count, 1)
        XCTAssertTrue(sent.isEmpty)
        XCTAssertTrue(notices.isEmpty)
    }

    func testFinishedRunsLeaveAfterTenMinutesAndClearAllHidesThem() {
        appendStart()
        let w = makeWatcher()
        w.tick(agents: [agent("s1", .completed)])
        clock = clock.addingTimeInterval(60)
        appendFinish(at: clock)
        w.runsFileChanged()
        XCTAssertEqual(items.count, 1)
        w.clearFinished()
        XCTAssertTrue(items.isEmpty)

        let w2 = makeWatcher()
        w2.tick(agents: [agent("s1", .completed)])
        XCTAssertEqual(items.count, 1)
        clock = clock.addingTimeInterval(601)
        w2.tick(agents: [agent("s1", .completed)])
        XCTAssertTrue(items.isEmpty)
    }

    func testAWorktreeSessionFindsTheMainRepositoryFile() throws {
        let wt = root.appendingPathComponent("wt")
        try FileManager.default.createDirectory(at: repo.appendingPathComponent(".git/worktrees/wt"), withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: wt, withIntermediateDirectories: true)
        try "../..\n".write(to: repo.appendingPathComponent(".git/worktrees/wt/commondir"), atomically: true, encoding: .utf8)
        try "gitdir: \(repo.path)/.git/worktrees/wt\n".write(to: wt.appendingPathComponent(".git"), atomically: true, encoding: .utf8)
        appendStart()
        let w = makeWatcher()
        w.tick(agents: [agent("s1", .working, cwd: wt.path)])
        XCTAssertEqual(items.count, 1)
    }

    func testMachinesRepositoriesAreWatchedWithoutASession() {
        appendStart()
        let w = makeWatcher(extra: [repo.path])
        w.tick(agents: [])
        XCTAssertTrue(w.hasActive)
    }

    func testTruncatedFileStartsOver() {
        appendStart()
        let w = makeWatcher()
        w.tick(agents: [agent("s1", .working)])
        XCTAssertEqual(items.count, 1)
        try? "".write(to: runsFile, atomically: true, encoding: .utf8)
        w.runsFileChanged()
        XCTAssertTrue(items.isEmpty)
    }

    // MARK: Sending

    func appendOtherRun(_ n: Int, session: String, finishedAt: Date) {
        let other = root.appendingPathComponent("logs/n-\(n).log").path
        append(#"{"event":"start","key":"just perf","log":"\#(other)","cwd":"\#(repo.path)","pid":99,"session":"\#(session)","limit":7200,"started_at":"\#(iso(finishedAt.addingTimeInterval(-30)))"}"# + "\n")
        append(#"{"key":"just perf","cmd":"x","seconds":60,"rc":\#(n),"head":"h","clean":true,"log":"\#(other)","at":"\#(iso(finishedAt))"}"# + "\n")
    }

    func testSeveralRunsEndedForOneSessionAreOneMessage() {
        appendStart()
        let w = makeWatcher()
        w.tick(agents: [agent("s1", .completed)])
        clock = clock.addingTimeInterval(60)
        appendFinish(rc: 0, at: clock)
        appendOtherRun(2, session: "s1", finishedAt: clock.addingTimeInterval(1))
        w.runsFileChanged()
        clock = clock.addingTimeInterval(RunWaker.grace + 5)
        w.tick(agents: [agent("s1", .completed)])
        XCTAssertEqual(sent.count, 1)
        XCTAssertEqual(sent[0].text.split(separator: "\n").count, 2)
    }

    func testNoSecondSendGoesToAJobWhileOneIsInFlight() {
        var held: [() -> Void] = []
        let w = RunWatcher(now: { self.clock }, pidAlive: { _ in true }, load: { 1 }, extraRepos: { [] }, enabled: { true },
                           send: { self.sent.append(($0, $1)) }, notify: { self.notices.append(($0, $1)) },
                           dispatch: { held.append($0) }, watchFiles: false)
        appendStart()
        w.tick(agents: [agent("s1", .completed)])
        clock = clock.addingTimeInterval(60)
        appendFinish(at: clock)
        w.runsFileChanged()
        clock = clock.addingTimeInterval(RunWaker.grace + 1)
        w.tick(agents: [agent("s1", .completed)])
        XCTAssertEqual(held.count, 1)

        appendOtherRun(2, session: "s1", finishedAt: clock.addingTimeInterval(-5))
        w.runsFileChanged()
        clock = clock.addingTimeInterval(RunWaker.grace + 1)
        w.tick(agents: [agent("s1", .completed)])
        w.tick(agents: [agent("s1", .completed)])
        XCTAssertEqual(held.count, 1, "the first send has not returned: nothing else goes to its job")
        XCTAssertTrue(sent.isEmpty)

        held.removeFirst()()
        XCTAssertEqual(sent.count, 1)
        settle()
        XCTAssertEqual(held.count, 1, "the waiting run goes out once the first one is done")
        held.removeFirst()()
        XCTAssertEqual(sent.count, 2)
        XCTAssertEqual(sent[1].job, "job-s1")
        XCTAssertTrue(sent[1].text.contains("exit 2"))
    }

    func testSendsToDifferentJobsRunOneAfterAnother() {
        let lock = NSLock()
        var running = 0, most = 0, done = 0
        let both = expectation(description: "both sent")
        both.expectedFulfillmentCount = 2
        let w = RunWatcher(now: { self.clock }, pidAlive: { _ in true }, load: { 1 }, extraRepos: { [] }, enabled: { true },
                           send: { _, _ in
                               lock.lock(); running += 1; most = max(most, running); lock.unlock()
                               Thread.sleep(forTimeInterval: 0.15)
                               lock.lock(); running -= 1; done += 1; lock.unlock()
                               both.fulfill()
                           },
                           notify: { self.notices.append(($0, $1)) }, watchFiles: false)
        appendStart(session: "s1")
        w.tick(agents: [agent("s1", .completed), agent("s2", .completed)])
        clock = clock.addingTimeInterval(60)
        appendFinish(at: clock)
        appendOtherRun(2, session: "s2", finishedAt: clock)
        w.runsFileChanged()
        clock = clock.addingTimeInterval(RunWaker.grace + 1)
        w.tick(agents: [agent("s1", .completed), agent("s2", .completed)])
        wait(for: [both], timeout: 5)
        XCTAssertEqual(most, 1)
    }

    /// The real FSEvents path: nothing calls `runsFileChanged` and `tick` runs once.
    func testFSEventsDeliverAppendsToRunsJsonl() {
        appendStart(session: "terminal")
        let w = makeWatcher(watchFiles: true)
        w.tick(agents: [agent("s1", .completed)])
        XCTAssertEqual(items.count, 1)
        let seen = expectation(description: "finish seen")
        let previous = w.onChange
        w.onChange = { previous?($0); if $0.first?.kind == .finished { seen.fulfill() } }
        clock = clock.addingTimeInterval(30)
        appendFinish(at: clock)
        wait(for: [seen], timeout: 10)
        XCTAssertEqual(notices.count, 1)
        w.stop()
    }
}
